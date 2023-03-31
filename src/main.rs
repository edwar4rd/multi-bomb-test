use axum::extract::ws;
use axum::extract::State;

use axum::{routing::get, Router};

use multi_bomb_test::packet::*;

#[derive(Debug)]
enum GameUpdate {
    BombMoved(BombPosition),
    // the player is expected to send back a BombMoveAction as response
    BombReceived(tokio::sync::oneshot::Sender<BombMoveAction>),
}

#[derive(Clone)]
struct AppState {
    // Channel for a newly created websocket handler to ask for a game to join
    game_request_tx: tokio::sync::mpsc::Sender<
        tokio::sync::oneshot::Sender<(
            // bomb count is sent before hello packet
            BombCount,
            tokio::sync::oneshot::Sender<(
                // Newly connected client can suggest a position/ID for the player
                PreferredID,
                // Player data are only created after olleh packet
                tokio::sync::oneshot::Sender<(
                    PlayerID,
                    PlayerData,
                    tokio::sync::mpsc::Receiver<(BombIndex, GameUpdate)>,
                    tokio::sync::watch::Receiver<GameScoareboard>,
                    tokio::sync::mpsc::Sender<PlayerID>,
                )>,
            )>,
        )>,
    >,
}

fn random_player_data() -> (PlayerName, PlayerColor) {
    (
        format!("Player{:04X}", rand::random::<u16>(),),
        format!("#{:06X}", rand::random::<u32>() >> 8,),
    )
}

fn move_bomb<'a>(
    bomb_pos: BombIndex,
    players: &'a std::collections::BTreeSet<BombIndex>,
    player_move: BombMoveAction,
) -> BombIndex {
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

    println!("Requesting server connection for a new player to join...");
    let (first_result_tx, first_result_rx) = tokio::sync::oneshot::channel();
    state.game_request_tx.send(first_result_tx).await.unwrap();

    let (bomb_count, olleh_tx) = first_result_rx.await.unwrap();

    socket
        .send(ServerPacket::PacketHELLO(bomb_count).into())
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
            println!("A websocket connection sent a OLLEH response that's not a text message...");
            socket
                .send(axum::extract::ws::Message::Close(Option::None))
                .await
                .unwrap();
            return;
        }
    };

    let suggested_pos = match text_response.parse::<ClientPacket>() {
        Err(err) => {
            println!("A websocket connection sent a packet expected to be a OLLEH but failed parsing:\n\t{}", err);
            socket
                .send(axum::extract::ws::Message::Close(Option::None))
                .await
                .unwrap();
            return;
        }
        Ok(ClientPacket::PacketMOVE(_, _)) => {
            println!("A websocket connection sent a packet expected to be a OLLEH but is a MOVE");
            socket
                .send(axum::extract::ws::Message::Close(Option::None))
                .await
                .unwrap();
            return;
        }
        Ok(ClientPacket::PacketOLLEH(suggested_pos)) => suggested_pos,
    };

    println!("Requesting server connection for a new player to join...");
    let (request_result_tx, request_result_rx) = tokio::sync::oneshot::channel();
    olleh_tx.send((suggested_pos, request_result_tx)).unwrap();

    let (
        player_id,
        (player_name, player_color),
        mut update_receiver,
        mut scoreboard_receiver,
        player_leave_notify,
    ) = request_result_rx.await.unwrap();
    println!("Received server connection and player data for new player...");

    socket
        .send(ServerPacket::PacketNAME(player_name, player_color).into())
        .await
        .unwrap();

    let mut bomb_actions: Vec<Option<tokio::sync::oneshot::Sender<BombMoveAction>>> = Vec::new();
    bomb_actions.resize_with(bomb_count as usize, || Option::None);

    loop {
        tokio::select! {
            biased;

            packet = socket.recv() => {
                let packet = packet.unwrap();
                let packet = match packet {
                    Err(_) => {
                        println!("A websocket connection produced a error (probably abruptly closed)...");
                        break;
                    }
                    Ok(axum::extract::ws::Message::Close(_)) => {
                        println!("Client leaved...");
                        break;
                    }
                    Ok(axum::extract::ws::Message::Text(text)) => text,
                    Ok(_) => {
                        println!("Received unexpected non-text packet from client...");
                        break;
                    }
                };

                let packet = match packet.parse::<ClientPacket>() {
                    Err(err) => {
                        println!("A websocket connection sent a packet expected to be a MOVE but failed parsing:\n\t{}", err);
                        break;
                    }
                    Ok(packet) => packet,
                };

                match packet {
                    ClientPacket::PacketOLLEH(_) => {
                        println!("A websocket connection sent a packet expected to be a MOVE but is a OLLEH");
                        break;
                    }
                    ClientPacket::PacketMOVE(index, action) => {
                        if index >= bomb_count {
                            println!("A websocket connection sent a MOVE packet with a index out of bound");
                            break;
                        }
                        match &bomb_actions[index as usize] {
                            None => {
                                println!("A websocket connection sent a MOVE packet while not holding the specified bomb");
                                break;
                            }
                            _ => {}
                        }
                        bomb_actions[index as usize].take().unwrap().send(action).unwrap();
                    }
                }
            }
            update = update_receiver.recv() => {
                let (index, update) = update.unwrap();
                match update {
                    GameUpdate::BombMoved(position) => {
                        socket.send(ServerPacket::PacketSTATUS(index, position.clone()).into()).await.unwrap();
                        bomb_actions[index as usize] = None;
                    }
                    GameUpdate::BombReceived(action_sender) => {
                        socket.send(ServerPacket::PacketSTATUS(index, BombPosition::X).into()).await.unwrap();
                        bomb_actions[index as usize] = Some(action_sender);
                    },
                }
            }

            _ = scoreboard_receiver.changed() => {
                let board = {
                    scoreboard_receiver.borrow().to_string()
                };
                socket.send(ServerPacket::PacketBOARD(board).into()).await.unwrap();
            }
        };
    }

    // this part SHOULD be optional after the problem is fixed
    // for mut action_tx in bomb_actions {
        // if let Some(action_tx) = action_tx.take() {
            // action_tx.send(BombMoveAction::R1).unwrap();
        // }
    // }

    player_leave_notify.send(player_id).await.unwrap();
    let _ = socket
        .send(axum::extract::ws::Message::Close(Option::None))
        .await;
    return;
}

async fn game_server(
    mut game_request_rx: tokio::sync::mpsc::Receiver<
        tokio::sync::oneshot::Sender<(
            // bomb count is sent before hello packet
            BombCount,
            tokio::sync::oneshot::Sender<(
                // Newly connected client can suggest a position/ID for the player
                PreferredID,
                // Player data are only created after olleh packet
                tokio::sync::oneshot::Sender<(
                    PlayerID,
                    PlayerData,
                    tokio::sync::mpsc::Receiver<(BombIndex, GameUpdate)>,
                    tokio::sync::watch::Receiver<GameScoareboard>,
                    tokio::sync::mpsc::Sender<PlayerID>,
                )>,
            )>,
        )>,
    >,
    bomb_count: BombCount,
) {
    println!("Server Started");

    let mut wait_olleh = tokio::task::JoinSet::new();
    loop {
        // only insert/delete when players join or leave
        // a set of all players (for calculating new bomb position)
        let mut players = std::collections::BTreeSet::<PlayerID>::new();
        // player id -> player name + color
        let mut players_data = std::collections::BTreeMap::<PlayerID, PlayerData>::new();

        // player id -> player score
        let mut players_score = std::collections::BTreeMap::<PlayerID, GameScore>::new();

        let mut bomb_pos = Vec::new();

        let mut players_channel = std::collections::BTreeMap::<
            PlayerID,
            tokio::sync::mpsc::Sender<(u32, GameUpdate)>,
        >::new();

        let (scoreboard_watch_tx, scoreboard_watch_rx) =
            tokio::sync::watch::channel("".to_string());
        let (player_leave_notify_tx, mut player_leave_notify_rx) = tokio::sync::mpsc::channel(32);

        let mut wait_bomb_action = tokio::task::JoinSet::new();

        let mut debug_tolerable_task;
        loop {
            tokio::select! {
                biased;

                new_request = game_request_rx.recv() => {
                    let new_request = new_request.unwrap();
                    let (wait_olleh_tx, wait_olleh_rx) = tokio::sync::oneshot::channel();
                    new_request.send((bomb_count, wait_olleh_tx)).unwrap();
                    wait_olleh.spawn(async move { wait_olleh_rx.await });
                }

                ollehed_request = wait_olleh.join_next(), if wait_olleh.len() > 0 => {
                    match ollehed_request.unwrap().unwrap() {
                        Err(_) => {
                            println!("A game request closed before returning OLLEH result...");
                        }
                        Ok((new_player_id, request_response)) => {
                            println!("A player joined...");
                            bomb_pos.resize(bomb_count as usize, new_player_id);
                            let new_player_data = random_player_data();
                            players.insert(new_player_id);
                            players_data.insert(new_player_id, new_player_data.clone());
                            players_score.insert(new_player_id, 0);
                            let (new_player_status_tx, new_player_status_rx) = tokio::sync::mpsc::channel(4);
                            scoreboard_watch_tx.send_replace(format!(
                                "{}\n0\n",
                                format!("{}\n{}", new_player_data.0, new_player_data.1)
                            ));
                            
                            request_response
                                .send((
                                    new_player_id,
                                    new_player_data,
                                    new_player_status_rx,
                                    scoreboard_watch_rx.clone(),
                                    player_leave_notify_tx.clone(),
                                ))
                                .unwrap();
                            debug_tolerable_task = bomb_count;
                            for bomb_index in 0..bomb_count {
                                let (action_tx, action_rx) = tokio::sync::oneshot::channel();
                                let send_start = tokio::time::Instant::now();
                                new_player_status_tx
                                    .send((bomb_index, GameUpdate::BombReceived(action_tx)))
                                    .await
                                    .unwrap();
                                wait_bomb_action.spawn(async move { (bomb_index, send_start, action_rx.await) });
                            }
                            players_channel.insert(new_player_id, new_player_status_tx);
                            break;
                        }
                    }
                }
            }
        }

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

                    for bomb_index in 0..bomb_count {
                        // assert_ne!(bomb_pos[bomb_index as usize], leaved_player);
                        if bomb_pos[bomb_index as usize] == leaved_player {
                        bomb_pos[bomb_index as usize] =
                                move_bomb(bomb_pos[bomb_index as usize], &players, BombMoveAction::R1);
                            for (player_id, channel) in &players_channel {
                                if *player_id < bomb_pos[bomb_index as usize] {
                                    channel
                                        .send((bomb_index, GameUpdate::BombMoved(BombPosition::R)))
                                        .await
                                        .unwrap();
                                }
                                if bomb_pos[bomb_index as usize] < *player_id {
                                    channel
                                        .send((bomb_index, GameUpdate::BombMoved(BombPosition::L)))
                                        .await
                                        .unwrap();
                                }
                            }
                            let (action_tx, action_rx) = tokio::sync::oneshot::channel();
                            let send_start = tokio::time::Instant::now();
                            players_channel[&bomb_pos[bomb_index as usize]]
                                .send((bomb_index, GameUpdate::BombReceived(action_tx)))
                                .await
                                .unwrap();
                            wait_bomb_action
                                .spawn(async move { (bomb_index, send_start, action_rx.await) });
                            debug_tolerable_task += 1;
                        }
                    }
                    
                    scoreboard_watch_tx.send_replace({
                        let mut scoreboard_map = std::collections::BTreeMap::new();

                        for (player_id, score) in &players_score {
                            scoreboard_map
                                .insert((score, player_id), (&players_data[&player_id], score));
                        }

                        let mut scoreboard_string = String::new();
                        for (_, (data, score)) in scoreboard_map.into_iter().rev() {
                            scoreboard_string.push_str(&format!("{}\n{}", data.0, data.1));
                            scoreboard_string.push_str(&format!("\n{score}\n"));
                        }
                        scoreboard_string
                    });
                }

                action_result = wait_bomb_action.join_next(), if wait_bomb_action.len() > 0 => {
                    let (bomb_index, send_start, action) = action_result.unwrap().unwrap();
                    let move_time = (tokio::time::Instant::now() - send_start).as_millis() as i32;
                    match action {
                        Err(_) => {
                            debug_tolerable_task -= 1;
                            println!("Player leaved before moving bomb");
                        }
                        Ok(action) => {
                            let move_score = if 4000 > move_time {
                                4100 - move_time
                            } else {
                                0
                            };
                            players_score.insert(
                                bomb_pos[bomb_index as usize],
                                players_score
                                    .get(&bomb_pos[bomb_index as usize])
                                    .unwrap_or(&0)
                                    + move_score as u32,
                            );
                            println!("{} got {move_score} points!", bomb_pos[bomb_index as usize]);
                            scoreboard_watch_tx.send_replace({
                                let mut scoreboard_map = std::collections::BTreeMap::new();

                                for (player_id, score) in &players_score {
                                    scoreboard_map
                                        .insert((score, player_id), (&players_data[&player_id], score));
                                }

                                let mut scoreboard_string = String::new();
                                for (_, (data, score)) in scoreboard_map.into_iter().rev() {
                                    scoreboard_string.push_str(&format!("{}\n{}", data.0, data.1));
                                    scoreboard_string.push_str(&format!("\n{score}\n"));
                                }
                                scoreboard_string
                            });
                            bomb_pos[bomb_index as usize] =
                                move_bomb(bomb_pos[bomb_index as usize], &players, action);
                            for (player_id, channel) in &players_channel {
                                if *player_id < bomb_pos[bomb_index as usize] {
                                    channel
                                        .send((bomb_index, GameUpdate::BombMoved(BombPosition::R)))
                                        .await
                                        .unwrap();
                                }
                                if bomb_pos[bomb_index as usize] < *player_id {
                                    channel
                                        .send((bomb_index, GameUpdate::BombMoved(BombPosition::L)))
                                        .await
                                        .unwrap();
                                }
                            }
                            let (action_tx, action_rx) = tokio::sync::oneshot::channel();
                            let send_start = tokio::time::Instant::now();
                            players_channel[&bomb_pos[bomb_index as usize]]
                                .send((bomb_index, GameUpdate::BombReceived(action_tx)))
                                .await
                                .unwrap();
                            wait_bomb_action
                                .spawn(async move { (bomb_index, send_start, action_rx.await) });
                        }
                    }
                }

                ollehed_request = wait_olleh.join_next(), if wait_olleh.len() > 0 => {
                    match ollehed_request.unwrap().unwrap() {
                        Err(_) => {
                            println!("A game request closed before returning OLLEH result...");
                        }
                        Ok((preferred_id, request_response)) => {
                            println!("A new player joined...");
                            let new_player_id = if players.contains(&preferred_id) {
                                *players.last().unwrap() + 1
                            } else {
                                preferred_id
                            };

                            let new_player_data = random_player_data();

                            players.insert(preferred_id);
                            players_data.insert(new_player_id, new_player_data.clone());
                            players_score.insert(new_player_id, 0);
                            let (new_player_status_tx, new_player_status_rx) =
                            tokio::sync::mpsc::channel(4);

                            scoreboard_watch_tx.send_replace({
                                let mut scoreboard_map = std::collections::BTreeMap::new();

                                for (player_id, score) in &players_score {
                                    scoreboard_map
                                        .insert((score, player_id), (&players_data[&player_id], score));
                                }

                                let mut scoreboard_string = String::new();
                                for (_, (data, score)) in scoreboard_map.into_iter().rev() {
                                    scoreboard_string.push_str(&format!("{}\n{}", data.0, data.1));
                                    scoreboard_string.push_str(&format!("\n{score}\n"));
                                }
                                scoreboard_string
                            });

                            request_response
                                .send((
                                    new_player_id,
                                    new_player_data,
                                    new_player_status_rx,
                                    scoreboard_watch_rx.clone(),
                                    player_leave_notify_tx.clone(),
                                ))
                                .unwrap();
                            for bomb_index in 0..bomb_count {
                                new_player_status_tx
                                    .send((
                                        bomb_index,
                                        if bomb_pos[bomb_index as usize] < new_player_id {
                                            GameUpdate::BombMoved(BombPosition::L)
                                        } else {
                                            GameUpdate::BombMoved(BombPosition::R)
                                        },
                                    ))
                                    .await
                                    .unwrap();
                            }
                            scoreboard_watch_tx.send_replace({
                                let mut scoreboard_map = std::collections::BTreeMap::new();

                                for (player_id, score) in &players_score {
                                    scoreboard_map
                                        .insert((score, player_id), (&players_data[&player_id], score));
                                }

                                let mut scoreboard_string = String::new();
                                for (_, (data, score)) in scoreboard_map.into_iter().rev() {
                                    scoreboard_string.push_str(&format!("{}\n{}", data.0, data.1));
                                    scoreboard_string.push_str(&format!("\n{score}\n"));
                                }
                                scoreboard_string
                            });
                            players_channel.insert(new_player_id, new_player_status_tx);
                        }
                    }
                }

                new_request = game_request_rx.recv() => {
                    let new_request = new_request.unwrap();
                    let (wait_olleh_tx, wait_olleh_rx) = tokio::sync::oneshot::channel();
                    new_request.send((bomb_count, wait_olleh_tx)).unwrap();
                    wait_olleh.spawn(async move { wait_olleh_rx.await });
                }

            }
            assert_eq!(debug_tolerable_task as usize, wait_bomb_action.len());
        }
    }
}
//
#[tokio::main]
async fn main() {
    let (game_request_tx, game_request_rx) = tokio::sync::mpsc::channel::<
        tokio::sync::oneshot::Sender<(
            // bomb count is sent before hello packet
            BombCount,
            tokio::sync::oneshot::Sender<(
                // Newly connected client can suggest a position/ID for the player
                PreferredID,
                // Player data are only created after olleh packet
                tokio::sync::oneshot::Sender<(
                    PlayerID,
                    (PlayerName, PlayerColor),
                    tokio::sync::mpsc::Receiver<(BombIndex, GameUpdate)>,
                    tokio::sync::watch::Receiver<GameScoareboard>,
                    tokio::sync::mpsc::Sender<PlayerID>,
                )>,
            )>,
        )>,
    >(32);

    //let shared_state = std::sync::Arc::new();
    let shared_state = AppState { game_request_tx };

    tokio::spawn(async move { game_server(game_request_rx, 5).await });

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
