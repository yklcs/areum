use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread::{self, JoinHandle},
};

use anyhow::Context;
use axum::{
    extract::Request,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing, Router,
};

use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use url::Url;

use crate::{env::Env, page::Page, vfs::VFSys};

pub struct Server {
    router: Router,
    vfs: VFSys,
    rx_cmd: broadcast::Receiver<Command>,
}

#[derive(Clone, Copy)]
pub enum Command {
    Stop,
    Restart,
}

struct Message {
    request: Url,
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
                    Some(Message { responder, request}) = rx_job.recv() => {
                        let mut page = Page::new(&mut env, &request).await?;

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
                            request.to_string()
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

        rt.block_on(future).unwrap();
    });

    (join_handle, tx_job, tx_stop)
}

impl Server {
    pub fn new(root: &Path) -> Result<(Self, broadcast::Sender<Command>), anyhow::Error> {
        let root = root.to_path_buf().canonicalize()?;
        let mut vfs = VFSys::new(&root)?;
        vfs.scan()?;

        let (mut handle, tx_job, mut tx_stop) = spawn_env(&root);

        let tx_job = Arc::new(Mutex::new(tx_job));
        let new_handler = |root: PathBuf, tx_job: Arc<Mutex<mpsc::Sender<Message>>>| {
            |request| get_page(request, root, tx_job)
        };

        let router = Router::new();
        let router = router.route("/", routing::get(new_handler(root.clone(), tx_job.clone())));
        let router = router.route(
            "/*path",
            routing::get(new_handler(root.clone(), tx_job.clone())),
        );

        let (tx_cmd, rx_cmd) = broadcast::channel(1);

        let mut rx_cmd_ = tx_cmd.subscribe();
        tokio::spawn(async move {
            loop {
                match rx_cmd_.recv().await.unwrap() {
                    Command::Restart => {
                        tx_stop.send(true).await.unwrap();
                        let (handle_, tx_job_, tx_stop_) = spawn_env(&root);

                        *tx_job.lock().await = tx_job_;
                        drop(tx_stop);
                        handle.join().unwrap();

                        handle = handle_;
                        tx_stop = tx_stop_;
                    }
                    Command::Stop => {
                        tx_stop.send(true).await.unwrap();

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
            vfs,
        };
        Ok((server, tx_cmd))
    }

    pub async fn serve(self, address: &str) -> Result<(), anyhow::Error> {
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
    root: PathBuf,
    tx: Arc<Mutex<mpsc::Sender<Message>>>,
) -> Result<impl IntoResponse, ServerError> {
    let path = root.join(".".to_string() + request.uri().path());

    if path.is_file() {
        return Ok(fs::read(path)?.into_response());
    }

    let paths_maybe = &[
        path.join("index.tsx"),
        path.join("index.jsx"),
        path.join("index.mdx"),
        path.join("index.md"),
        path.with_extension("tsx"),
        path.with_extension("jsx"),
        path.with_extension("mdx"),
        path.with_extension("md"),
    ];

    let path = paths_maybe
        .iter()
        .find(|p| p.is_file())
        .context("could not find file")?;
    let url = Url::from_file_path(path).unwrap();

    let (tx_page, rx_page) = oneshot::channel();
    tx.lock()
        .await
        .send(Message {
            request: url,
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
