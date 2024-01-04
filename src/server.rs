use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread::{self, JoinHandle},
};

use anyhow::anyhow;
use axum::{
    extract::Request,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing, Router,
};

use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use url::Url;

use crate::{env::Env, page::Page, src_fs::SrcFs};

pub struct Server {
    router: Router,
    src_fs: SrcFs,
    rx_cmd: broadcast::Receiver<Command>,
}

#[derive(Clone, Copy)]
pub enum Command {
    Stop,
    Restart,
}

struct Message {
    url: Url,
    path: PathBuf,
    responder: oneshot::Sender<Result<Page, anyhow::Error>>,
}

fn spawn_env(root: &PathBuf) -> (JoinHandle<()>, mpsc::Sender<Message>, mpsc::Sender<bool>) {
    let (tx_job, mut rx_job) = mpsc::channel(16);
    let (tx_stop, mut rx_stop) = mpsc::channel::<bool>(1);
    let root = root.clone();

    let join_handle = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut env: Env = Env::new(&root).unwrap();

        let future = async {
            env.bootstrap().await?;

            loop {
                tokio::select! {
                    Some(Message { responder, url, path}) = rx_job.recv() => {
                        let mut page = match Page::new(&mut env, &url, &path).await {
                            Ok(page) => page,
                            Err(err) => {
                                responder.send(Err(anyhow!("{}", err))).unwrap_or_else(|_| panic!("error sending to channel"));
                                return Err(err);
                            }
                        };

                        env.bundler.clear();
                        env.bundler.push(format!(
                            r#"import {{ runScript }} from "{}"
                            "#,
                            &Url::from_file_path(root.join("/areum/jsx-runtime"))
                                .unwrap()
                                .to_string()
                        ));
                        env.bundler.push(format!(
                            r#"
                            import {{ default as Page }} from "{}"
                            if (!("Deno" in window)) {{
                                if (Page.script) {{
                                    Page.script()
                                }}
                                runScript(Page())
                            }}
                            "#,
                            url.to_string()
                        ));

                        page.script = env.bundle().await?;

                        responder.send(Ok(page)).unwrap_or_else(|_| panic!("error sending to channel"));
                    },
                    _ = rx_stop.recv() => {
                        break;
                    }
                }
            }

            Ok::<(), anyhow::Error>(())
        };

        if let Err(err) = rt.block_on(future) {
            eprintln!("{}", err);
        };
    });

    (join_handle, tx_job, tx_stop)
}

impl Server {
    pub fn new(root: &Path) -> Result<(Self, broadcast::Sender<Command>), anyhow::Error> {
        let root = root.to_path_buf().canonicalize()?;
        let src_fs = SrcFs::new(&root);

        let (mut handle, tx_job, mut tx_stop) = spawn_env(&root);

        let tx_job = Arc::new(Mutex::new(tx_job));
        let new_handler = |src_fs: SrcFs, tx_job: Arc<Mutex<mpsc::Sender<Message>>>| {
            |request| get_page(request, src_fs, tx_job)
        };

        let router = Router::new();
        let router = router.route(
            "/",
            routing::get(new_handler(src_fs.clone(), tx_job.clone())),
        );
        let router = router.route(
            "/*path",
            routing::get(new_handler(src_fs.clone(), tx_job.clone())),
        );

        let (tx_cmd, rx_cmd) = broadcast::channel(16);

        let mut rx_cmd_ = tx_cmd.subscribe();
        let src_fs_ = src_fs.clone();
        tokio::spawn(async move {
            loop {
                match rx_cmd_.recv().await.unwrap() {
                    Command::Restart => {
                        let _ = tx_stop.send(true).await;
                        let (handle_, tx_job_, tx_stop_) = spawn_env(&root);
                        src_fs_.scan().await.unwrap();

                        *tx_job.lock().await = tx_job_;
                        drop(tx_stop);
                        handle.join().unwrap();

                        handle = handle_;
                        tx_stop = tx_stop_;
                    }
                    Command::Stop => {
                        let _ = tx_stop.send(true).await;

                        drop(tx_job);
                        drop(tx_stop);
                        handle.join().unwrap();

                        break;
                    }
                }
            }
        });

        let server = Server {
            router,
            rx_cmd,
            src_fs,
        };
        Ok((server, tx_cmd))
    }

    pub async fn serve(self, address: &str) -> Result<(), anyhow::Error> {
        self.src_fs.scan().await?;
        let listener = tokio::net::TcpListener::bind(address).await?;
        axum::serve(listener, self.router)
            .with_graceful_shutdown(async move {
                loop {
                    match self.rx_cmd.resubscribe().recv().await.unwrap() {
                        Command::Stop => {
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .await?;
        Ok(())
    }
}

async fn get_page(
    request: Request,
    src_fs: SrcFs,
    tx: Arc<Mutex<mpsc::Sender<Message>>>,
) -> Result<impl IntoResponse, ServerError> {
    let abspath = request.uri().path();
    let relpath = abspath.strip_prefix("/").unwrap_or(abspath);

    if let Some(file) = src_fs.find(relpath).await {
        return Ok(src_fs.read(&file)?.into_response());
    }

    let (url, path) = if let Some(file) = src_fs.find_page_src(relpath).await {
        (
            Url::from_file_path(&file.path).unwrap(),
            src_fs.site_path(&file).await?,
        )
    } else {
        return Err(anyhow!("could not find page").into());
    };

    let (tx_page, rx_page) = oneshot::channel();
    tx.lock()
        .await
        .send(Message {
            url,
            path,
            responder: tx_page,
        })
        .await
        .unwrap();

    let page = rx_page.await?;
    let html = page?.render_to_string()?;

    Ok(Html(html).into_response())
}

struct ServerError(anyhow::Error);

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

impl<E> From<E> for ServerError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
