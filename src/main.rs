use axum::extract::ws;
use axum::extract::State;

use axum::{routing::get, Router};

#[derive(Debug)]
enum BombMoveAction {
    L3,
    L1,
    R1,
    R2,
}

impl std::str::FromStr for BombMoveAction {
    type Err = ();

    fn from_str(input: &str) -> Result<BombMoveAction, Self::Err> {
        match input {
            "L3" => Ok(BombMoveAction::L3),
            "L1" => Ok(BombMoveAction::L1),
            "R1" => Ok(BombMoveAction::R1),
            "R2" => Ok(BombMoveAction::R2),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
enum BombPosition {
    L,
    R,
}

#[derive(Debug)]
enum GameUpdate {
    BombMoved(BombPosition),
    BombReceived(tokio::sync::oneshot::Sender<BombMoveAction>),
}

#[derive(Clone)]
struct AppState {
    // Channel for a newly created websocket handler to ask for a game to join
    game_request_tx: tokio::sync::mpsc::Sender<(
        // Newly connected client can suggest a position for the player
        u32,
        // Stuff that handler for the client expect to receive
        tokio::sync::oneshot::Sender<(
            // internal id for the player/handler (currently only used for signaling that the player has leaved)
            u32,
            String,
            tokio::sync::mpsc::Receiver<GameUpdate>,
            tokio::sync::watch::Receiver<String>,
            tokio::sync::mpsc::Sender<u32>,
        )>,
    )>,
}

fn random_player_data() -> String {
    format!(
        "{:04X}\n{}",
        rand::random::<u16>(),
        random_color::RandomColor::new().to_hex()
    )
}

fn move_bomb<'a>(
    bomb_pos: u32,
    players: &'a std::collections::BTreeSet<u32>,
    player_move: BombMoveAction,
) -> u32 {
    // assert!(players.contains(&bomb_pos));
    match player_move {
        BombMoveAction::R1 => {
            let mut right_range = players.range((
                std::ops::Bound::Excluded(bomb_pos),
                std::ops::Bound::Unbounded,
            ));
            match right_range.next() {
                Some(next) => *next,
                None => *players.first().unwrap(),
            }
        }
        BombMoveAction::L1 => {
            let mut left_range = players.range((
                std::ops::Bound::Unbounded,
                std::ops::Bound::Excluded(bomb_pos),
            ));
            match left_range.next_back() {
                Some(next) => *next,
                None => *players.last().unwrap(),
            }
        }
        BombMoveAction::L3 => move_bomb(
            move_bomb(
                move_bomb(bomb_pos, players, BombMoveAction::L1),
                players,
                BombMoveAction::L1,
            ),
            players,
            BombMoveAction::L1,
        ),
        BombMoveAction::R2 => move_bomb(
            move_bomb(bomb_pos, players, BombMoveAction::R1),
            players,
            BombMoveAction::R1,
        ),
    }
}

async fn ws_get_handler(
    ws: ws::WebSocketUpgrade,
    State(state): State<AppState>,
) -> axum::response::Response {
    ws.on_upgrade(|socket| ws_client_handler(socket, state))
}

async fn ws_client_handler(mut socket: ws::WebSocket, state: AppState) {
    println!("New websocket connection has established...");
    socket
        .send(axum::extract::ws::Message::Text("hello\n1".into()))
        .await
        .unwrap();
    let response = match tokio::time::timeout(tokio::time::Duration::from_secs(10), socket.recv())
        .await
    {
        Err(_) => {
            println!("A websocket connection took too long to send a OLLEH response...");
            socket
                .send(axum::extract::ws::Message::Close(Option::None))
                .await
                .unwrap();
            return;
        }
        Ok(None) => {
            println!("A websocket connection abruptly closed before sending a OLLEH response...");
            socket
                .send(axum::extract::ws::Message::Close(Option::None))
                .await
                .unwrap();
            return;
        }
        Ok(Some(Err(_))) => {
            println!("A websocket connection caused a error before sending a OLLEH response...");
            socket
                .send(axum::extract::ws::Message::Close(Option::None))
                .await
                .unwrap();
            return;
        }
        Ok(Some(Ok(response))) => response,
    };

    let text_response = match response {
        ws::Message::Text(text_response) => text_response,
        _ => {
            println!("A websocket connection sended a OLLEH response that's not a text message...");
            socket
                .send(axum::extract::ws::Message::Close(Option::None))
                .await
                .unwrap();
            return;
        }
    };

    if text_response.len() <= 6 || &text_response[0..6] != "olleh\n" {
        println!("A websocket connection sended a OLLEH response that's not formatted properly...");
        socket
            .send(axum::extract::ws::Message::Close(Option::None))
            .await
            .unwrap();
        return;
    }

    let suggested_pos: u32 = match text_response[6..].parse() {
        Ok(num) => num,
        Err(_) => {
            println!("A websocket connection sended a OLLEH response containing a bad number...");
            socket
                .send(axum::extract::ws::Message::Close(Option::None))
                .await
                .unwrap();
            return;
        }
    };

    println!("Requesting server connection for a new player to join...");
    let (request_result_tx, request_result_rx) = tokio::sync::oneshot::channel();
    state
        .game_request_tx
        .send((suggested_pos, request_result_tx))
        .await
        .unwrap();
    let (player_id, player_data, mut update_receiver, scoreboard_receiver, player_leave_notify) =
        request_result_rx.await.unwrap();
    println!("Received server connection and player data for new player...");

    socket
        .send(axum::extract::ws::Message::Text(format!(
            "name\n{}",
            player_data
        )))
        .await
        .unwrap();

    loop {
        tokio::select! {
            biased;

            packet = socket.recv() => {
                player_leave_notify.send(player_id).await.unwrap();

                let packet = packet.unwrap();
                match packet {
                    Err(_) => {
                        println!("A websocket connection produced a error (probably abruptly closed)...");
                        player_leave_notify.send(player_id).await.unwrap();
                        return;
                    }
                    Ok(axum::extract::ws::Message::Close(_)) => {
                        println!("Client leaved...");
                    }
                    _ => {
                        socket
                            .send(axum::extract::ws::Message::Close(Option::None))
                            .await
                            .unwrap();
                        println!("Received unexpected packet from client...");
                    }
                }
                return;
            }
            update = update_receiver.recv() => {
                let update = update.unwrap();
                match update {
                    GameUpdate::BombMoved(position) => {
                        socket
                            .send(axum::extract::ws::Message::Text(format!(
                                "status\n0 {}",
                                match position {
                                    BombPosition::L => "L",
                                    BombPosition::R => "R",
                                }
                            )))
                            .await
                            .unwrap();
                        socket
                            .send(axum::extract::ws::Message::Text(format!(
                                "board\n{}",
                                scoreboard_receiver.borrow().to_string()
                            )))
                            .await
                            .unwrap();
                    }
                    GameUpdate::BombReceived(reaction_sender) => {
                        socket.send(axum::extract::ws::Message::Text("status\n0 X".into())).await.unwrap();
                        socket
                            .send(axum::extract::ws::Message::Text(format!(
                                "board\n{}",
                                scoreboard_receiver.borrow().to_string()
                            )))
                            .await
                            .unwrap();
                        let response = match socket.recv().await.unwrap() {
                            Err(_) => {
                                println!("A websocket connection produced a error (probably abruptly closed) before sending a MOVE response...");
                                player_leave_notify.send(player_id).await.unwrap();
                                return;
                            }
                            Ok(axum::extract::ws::Message::Close(_)) => {
                                println!("A websocket connection closed before sending a MOVE response...");
                                player_leave_notify.send(player_id).await.unwrap();
                                return;
                            }
                            Ok(axum::extract::ws::Message::Text(text)) => text,
                            _ => {
                                println!("A websocket connection sended a MOVE response that's not a text message...");
                                player_leave_notify.send(player_id).await.unwrap();
                                socket
                                    .send(axum::extract::ws::Message::Close(Option::None))
                                    .await
                                    .unwrap();
                               return;
                            }
                        };
                        if !response.starts_with("move\n0 ") || response.len() != 9 {
                            println!("A websocket connection sended a MOVE response that's not formatted properly...");
                            player_leave_notify.send(player_id).await.unwrap();
                            socket
                                .send(axum::extract::ws::Message::Close(Option::None))
                                .await
                                .unwrap();
                            return;
                        }
                        let player_move = match response[7..9].parse() {
                            Ok(player_move) => player_move,
                            Err(_) => {
                                println!("A websocket connection sended a MOVE response that's not formatted properly...");
                                player_leave_notify.send(player_id).await.unwrap();
                                socket
                                    .send(axum::extract::ws::Message::Close(Option::None))
                                    .await
                                    .unwrap();
                                return;
                            }
                        };
                        let _ = reaction_sender.send(player_move);
                    },
                }
            }
        };
    }
}

async fn game_server(
    mut game_request_rx: tokio::sync::mpsc::Receiver<(
        u32,
        tokio::sync::oneshot::Sender<(
            u32,
            String,
            tokio::sync::mpsc::Receiver<GameUpdate>,
            tokio::sync::watch::Receiver<String>,
            tokio::sync::mpsc::Sender<u32>,
        )>,
    )>,
) {
    println!("Server Started");
    loop {
        // Wait for a first player to start the game
        println!("Waiting for first player to join...");
        let (new_player_id, request_response_tx) = match game_request_rx.recv().await {
            Some((new_player_id, request_response_tx)) => (new_player_id, request_response_tx),
            None => {
                println!("Server Exited");
                break;
            }
        };
        println!("A player joined...");

        // only insert/delete when players join or leave
        // a set of all players (for calculating new bomb position)
        let mut players = std::collections::BTreeSet::<u32>::new();
        // player id -> player name + color
        let mut players_data = std::collections::BTreeMap::<u32, String>::new();

        // player id -> player score
        let mut players_score = std::collections::BTreeMap::<u32, u32>::new();

        let new_player_data = random_player_data();
        let mut bomb_pos = new_player_id;

        let mut players_channel =
            std::collections::BTreeMap::<u32, tokio::sync::mpsc::Sender<GameUpdate>>::new();
        let (scoreboard_watch_tx, scoreboard_watch_rx) =
            tokio::sync::watch::channel(format!("{}\n0\n", &new_player_data));
        let (player_leave_notify_tx, mut player_leave_notify_rx) = tokio::sync::mpsc::channel(32);

        players.insert(new_player_id);
        players_data.insert(new_player_id, new_player_data.clone());
        let (new_player_status_tx, new_player_status_rx) = tokio::sync::mpsc::channel(4);

        request_response_tx
            .send((
                new_player_id,
                new_player_data,
                new_player_status_rx,
                scoreboard_watch_rx.clone(),
                player_leave_notify_tx.clone(),
            ))
            .unwrap();

        let (action_tx, mut action_rx) = tokio::sync::oneshot::channel();
        let mut send_start = tokio::time::Instant::now();
        new_player_status_tx
            .send(GameUpdate::BombReceived(action_tx))
            .await
            .unwrap();
        players_channel.insert(new_player_id, new_player_status_tx);

        loop {
            tokio::select! {
                biased;

                leaved_player = player_leave_notify_rx.recv() => {
                    let leaved_player = leaved_player.unwrap();
                    players.remove(&leaved_player);
                    if players.len() == 0 {
                        println!("All player leaved...");
                        break;
                    }

                    players_channel.remove(&leaved_player);
                    players_data.remove(&leaved_player);
                    players_score.remove(&leaved_player);

                    if bomb_pos == leaved_player {
                        bomb_pos = move_bomb(bomb_pos, &players, BombMoveAction::R1);
                    }

                    for (player_id, channel) in &players_channel {
                        if *player_id < bomb_pos {
                            channel
                                .send(GameUpdate::BombMoved(BombPosition::R))
                                .await
                                .unwrap();
                        }
                        if bomb_pos < *player_id {
                            channel
                                .send(GameUpdate::BombMoved(BombPosition::L))
                                .await
                                .unwrap();
                        }
                    }

                    let (action_tx, new_action_rx) = tokio::sync::oneshot::channel();
                    action_rx = new_action_rx;
                    send_start = tokio::time::Instant::now();
                    players_channel[&bomb_pos]
                        .send(GameUpdate::BombReceived(action_tx))
                        .await
                        .unwrap();
                },
                player_move = &mut action_rx =>
                {
                    let player_move = player_move.unwrap();
                    let move_time = (tokio::time::Instant::now() - send_start).as_millis() as i32;
                    let move_score = if 8000 > move_time  {
                        8100 - move_time
                    } else {
                        0
                    };
                    players_score.insert(
                        bomb_pos,
                        players_score.get(&bomb_pos).unwrap_or(&0) + move_score as u32,
                    );
                    println!("{bomb_pos} got {move_score} points!");

                    scoreboard_watch_tx.send_replace({
                        let mut scoreboard_map = std::collections::BTreeMap::new();

                        for (player_id, score) in &players_score {
                            scoreboard_map.insert((score, player_id), (&players_data[&player_id], score));
                        }

                        let mut scoreboard_string = String::new();
                        for (_, (data, score)) in scoreboard_map.into_iter().rev() {
                            scoreboard_string.push_str(data);
                            scoreboard_string.push_str(&format!("\n{score}\n"));
                        }
                        scoreboard_string
                    });

                    bomb_pos = move_bomb(bomb_pos, &players, player_move);

                    for (player_id, channel) in &players_channel {
                        if *player_id < bomb_pos {
                            channel
                                .send(GameUpdate::BombMoved(BombPosition::R))
                                .await
                                .unwrap();
                        }
                        if bomb_pos < *player_id {
                            channel
                                .send(GameUpdate::BombMoved(BombPosition::L))
                                .await
                                .unwrap();
                        }
                    }

                    let (action_tx, new_action_rx) = tokio::sync::oneshot::channel();
                    action_rx = new_action_rx;
                    send_start = tokio::time::Instant::now();
                    players_channel[&bomb_pos]
                        .send(GameUpdate::BombReceived(action_tx))
                        .await
                        .unwrap();

                    // todo: handle player leave during the above
                },
                new_request = game_request_rx.recv() => {
                    let (mut new_player_id, request_response_tx) = new_request.unwrap();
                    if players.contains(&new_player_id) {
                        new_player_id = *players.last().unwrap()+1;
                    }

                    let new_player_data = random_player_data();

                    players.insert(new_player_id);
                    players_data.insert(new_player_id, new_player_data.clone());
                    let (new_player_status_tx, new_player_status_rx) = tokio::sync::mpsc::channel(4);

                    request_response_tx
                        .send((
                            new_player_id,
                            new_player_data,
                            new_player_status_rx,
                            scoreboard_watch_rx.clone(),
                            player_leave_notify_tx.clone(),
                        ))
                        .unwrap();
                    new_player_status_tx.send(if bomb_pos < new_player_id {
                        GameUpdate::BombMoved(BombPosition::L)
                    } else {
                        GameUpdate::BombMoved(BombPosition::L)
                    }).await.unwrap();
                    players_channel.insert(new_player_id, new_player_status_tx);
                }
            };
        }
    }
}
//
#[tokio::main]
async fn main() {
    let (game_request_tx, game_request_rx) = tokio::sync::mpsc::channel::<(
        u32,
        tokio::sync::oneshot::Sender<(
            u32,
            String,
            tokio::sync::mpsc::Receiver<GameUpdate>,
            tokio::sync::watch::Receiver<String>,
            tokio::sync::mpsc::Sender<u32>,
        )>,
    )>(32);

    //let shared_state = std::sync::Arc::new();
    let shared_state = AppState { game_request_tx };

    tokio::spawn(async move { game_server(game_request_rx).await });

    // build our application with a single route

    let assets_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");

    let app = Router::new()
        .fallback_service(axum::routing::get_service(
            tower_http::services::ServeDir::new(assets_dir).append_index_html_on_directories(true),
        ))
        .route("/ws", get(ws_get_handler))
        .with_state(shared_state);

    let _ = axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await;
}
