use std::{
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
use deno_core::{futures::FutureExt, v8};
use dongjak::runtime::{Runtime, RuntimeOptions};

use tokio::sync::{mpsc, oneshot, Mutex};
use url::Url;

use crate::{page::Page, site::Site};

pub struct Server {
    router: Router,
    pub tx_cmd: mpsc::Sender<Command>,
}

pub enum Command {
    Stop,
    Restart,
}

struct Message {
    request: Url,
    responder: oneshot::Sender<Result<Page, anyhow::Error>>,
}

fn spawn_runtime(
    root: &PathBuf,
) -> (
    JoinHandle<()>,
    mpsc::Sender<Message>,
    oneshot::Sender<Command>,
) {
    let (tx_job, mut rx_job) = mpsc::channel(16);
    let (tx_stop, rx_stop) = oneshot::channel::<Command>();
    let root = root.clone();

    let handle = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();

        let mut runtime = Runtime::new(
            &root,
            RuntimeOptions {
                jsx_import_source: "/areum".into(),
            },
        );

        let future = async {
            let jsx_mod = runtime
                .load_from_string(
                    &Url::from_file_path(runtime.root().join("/areum/jsx-runtime")).unwrap(),
                    include_str!("ts/jsx-runtime.ts"),
                    false,
                )
                .await?;
            runtime.eval(jsx_mod).await?;

            let loader_mod = runtime
                .load_from_string(
                    &Url::from_file_path(runtime.root().join("__loader.ts")).unwrap(),
                    include_str!("ts/loader.ts"),
                    false,
                )
                .await?;
            runtime.eval(loader_mod).await?;

            let loader = runtime
                .export::<v8::Function>(loader_mod, "default")
                .await?;

            runtime
                .functions
                .insert(Site::LOADER_FN_KEY.into(), loader.into());

            while let Some(Message { responder, request }) = rx_job.recv().await {
                let mut page = Page::new(&mut runtime, &request).await?;

                let bundle_url = Url::from_file_path(root.join("__index.ts")).unwrap();
                let mut bundle = String::new();
                bundle.push_str(&format!(
                    r#"import {{ runScript }} from "{}"
                    "#,
                    &Url::from_file_path(root.join("/areum/jsx-runtime"))
                        .unwrap()
                        .to_string()
                ));
                bundle.push_str(&format!(
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

                runtime.graph_loader.inject(bundle_url.clone(), bundle);
                runtime.add_root(&bundle_url).await;
                page.script = runtime.bundle(&bundle_url).await?;

                let _ = responder.send(Ok(page));
            }

            Ok::<(), anyhow::Error>(())
        };
        rt.block_on(future).unwrap();

        let handle = rt.handle().clone();
        handle.block_on(async move {
            rx_stop.await;
            rt.shutdown_background();
        });
    });

    (handle, tx_job, tx_stop)
}

impl Server {
    pub fn new(root: &Path) -> Result<Self, anyhow::Error> {
        let root = root.to_path_buf().canonicalize()?;
        let (mut handle, tx_job, mut tx_stop) = spawn_runtime(&root);

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

        let (tx_cmd, mut rx_cmd) = mpsc::channel::<Command>(1);

        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let future = async {
                while let Some(cmd) = rx_cmd.recv().await {
                    match cmd {
                        Command::Stop | Command::Restart => {
                            tx_stop.send(Command::Stop);
                            let (handle_, tx_job_, tx_stop_) = spawn_runtime(&root);
                            *tx_job.lock().await = tx_job_;
                            handle = handle_;
                            tx_stop = tx_stop_;
                        }
                    }
                }
            }
            .boxed_local();
            rt.block_on(future);
        });

        Ok(Server { router, tx_cmd })
    }

    pub async fn serve(self, address: &str) -> Result<(), anyhow::Error> {
        let listener = tokio::net::TcpListener::bind(address).await?;
        axum::serve(listener, self.router).await?;
        Ok(())
    }
}

async fn get_page(
    request: Request,
    root: PathBuf,
    tx: Arc<Mutex<mpsc::Sender<Message>>>,
) -> Result<Html<String>, ServerError> {
    let path = root.join(".".to_string() + request.uri().path());

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
    let _ = tx
        .lock()
        .await
        .send(Message {
            request: url,
            responder: tx_page,
        })
        .await;

    let page = rx_page.await?;
    let html = page?.render_to_string()?;

    Ok(Html(html))
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
