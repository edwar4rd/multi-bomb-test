#[derive(Debug)]
pub enum BombMoveAction {
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

impl std::fmt::Display for BombMoveAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                BombMoveAction::L3 => "L3",
                BombMoveAction::L1 => "L1",
                BombMoveAction::R1 => "R1",
                BombMoveAction::R2 => "R2",
            }
        )
    }
}

#[derive(Debug)]
pub enum BombPosition {
    L,
    X,
    R,
}

impl std::str::FromStr for BombPosition {
    type Err = ();

    fn from_str(input: &str) -> Result<BombPosition, Self::Err> {
        match input {
            "L" => Ok(BombPosition::L),
            "X" => Ok(BombPosition::X),
            "R" => Ok(BombPosition::R),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for BombPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                BombPosition::L => "L",
                BombPosition::X => "X",
                BombPosition::R => "R",
            }
        )
    }
}

pub type BombCount = u32;
pub type BombIndex = u32;
pub type PreferredID = u32;
pub type GameScore = u32;
pub type PlayerName = String;
pub type PlayerColor = String;
pub type GameScoareboard = String;

#[derive(Debug)]
enum ClientPacket {
    PacketOLLEH(PreferredID),
    PacketMOVE(BombIndex, BombMoveAction),
}

impl std::str::FromStr for ClientPacket {
    type Err = &'static str;

    fn from_str(input: &str) -> Result<ClientPacket, Self::Err> {
        if input.split("\n").count() != 2 {
            return Err("Packet has unexpected amount of lines");
        }

        let split: Vec<&str> = input.split("\n").collect();

        match split[0] {
            "olleh" => {
                let preferred_id = match split[1].parse() {
                    Ok(id) => id,
                    Err(_) => return Err("Packet contains a bad number"),
                };
                Ok(ClientPacket::PacketOLLEH(preferred_id))
            }
            "move" => {
                if input.split("\n").count() != 2 {
                    return Err("Packet has unexpected amount of lines");
                }

                if split[1].split(" ").count() != 2 {
                    return Err("Packet has unexpected amount of whitespaces");
                }

                let info_split: Vec<&str> = split[1].split(" ").collect();

                let bomb_index = match info_split[0].parse() {
                    Ok(index) => index,
                    Err(_) => return Err("Packet contains a bad number"),
                };

                let bomb_move_action = match info_split[1].parse() {
                    Ok(id) => id,
                    Err(_) => return Err("Packet contains a bad action type"),
                };

                Ok(ClientPacket::PacketMOVE(bomb_index, bomb_move_action))
            }
            _ => Err("Packet contained a unexpected type identifier"),
        }
    }
}

pub enum ServerPacket {
    PacketHELLO(BombCount),
    PacketNAME(PlayerName, PlayerColor),
    PacketSTATUS(BombIndex, BombPosition),
    PacketBOARD(GameScoareboard),
}

impl std::fmt::Display for ServerPacket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PacketHELLO(bomb_count) => {
                write!(f, "hello\n{}", bomb_count)
            }
            Self::PacketNAME(player_name, player_color) => {
                write!(f, "name\n{}\n{}", player_name, player_color)
            }
            Self::PacketSTATUS(bomb_index, bomb_position) => {
                write!(f, "status\n{} {}", bomb_index, bomb_position)
            }
            Self::PacketBOARD(game_scoreboard) => write!(f, "board\n{}", game_scoreboard),
        }
    }
}
