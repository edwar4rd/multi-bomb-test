use axum::extract::State;
use axum::{routing::get, Router};

#[derive(Clone)]
struct AppState {
    count_handle: tokio::sync::mpsc::Sender<tokio::sync::oneshot::Sender<u32>>,
}

#[axum_macros::debug_handler]
async fn count(State(state): State<AppState>) -> String {
    let (tx, rx) = tokio::sync::oneshot::channel();
    if let Err(_) = state.count_handle.send(tx).await {
        println!("Err: Receiver Dropped");
    }
    match rx.await {
        Ok(num) => format!("{num}\n"),
        Err(_) => "Couldn't get number!".to_string(),
    }
}

async fn count_server(mut rx: tokio::sync::mpsc::Receiver<tokio::sync::oneshot::Sender<u32>>) {
    println!("Server Started");
    let mut num = 0;
    loop {
        match rx.recv().await {
            Some(return_oneshot) => {
                return_oneshot.send(num).unwrap();
                num += 1;
            }
            None => {
                println!("Server Exited");
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let (tx, rx) = tokio::sync::mpsc::channel::<tokio::sync::oneshot::Sender<u32>>(32);

    //let shared_state = std::sync::Arc::new();
    let shared_state = AppState { count_handle: tx };

    tokio::spawn(async move { count_server(rx).await });

    // build our application with a single route

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/count", get(count))
        .with_state(shared_state);

    let _ = axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await;
}
