use std::{
    path::{Path, PathBuf},
    thread,
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

use tokio::sync::{mpsc, oneshot};
use url::Url;

use crate::{page::Page, site::Site};

pub struct Server {
    router: Router,
}

struct Message {
    request: Url,
    responder: oneshot::Sender<Result<Page, anyhow::Error>>,
}

impl Server {
    pub fn new(root: &Path) -> Result<Self, anyhow::Error> {
        let root = root.to_path_buf().canonicalize()?;
        let root2 = root.clone();
        let root3 = root.clone();

        let (tx, mut rx) = mpsc::channel::<Message>(16);
        thread::spawn(move || {
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

                while let Some(Message { responder, request }) = rx.recv().await {
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
            }
            .boxed_local();

            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(future).unwrap();
        });

        let tx_handler1 = tx.clone();
        let tx_handler2 = tx.clone();

        let router = Router::new();
        let router = router.route(
            "/",
            routing::get(move |request| get_page(request, root2, tx_handler1)),
        );
        let router = router.route(
            "/*path",
            routing::get(move |request| get_page(request, root3, tx_handler2)),
        );

        Ok(Server { router })
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
    tx: mpsc::Sender<Message>,
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
