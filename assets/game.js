let id = prompt("Please enter your preferred ID number", Math.floor(Math.random() * 200));

let url = new URL(window.location.href);
console.log((url.protocol == "http:" ? "ws:" : "wss:") + "//" + url.host + url.pathname + "ws");
let socket = new WebSocket((url.protocol == "http:" ? "ws:" : "wss:") + "//" + url.host + url.pathname + "ws");
let bomb_text = { "X": "💣️", "L": "<<", "R": ">>" };

let clickable = [false];

let scoreboard_item = [document.createElement("div"), document.createElement("label"), document.createElement("br")];
scoreboard_item[0].classList.add("game-players-data");
scoreboard_item[0].appendChild(document.createElement("div"));
scoreboard_item[0].children[0].classList.add("game-players-color");
scoreboard_item[0].appendChild(document.createElement("label"));
scoreboard_item[0].children[1].classList.add("game-players-name");
scoreboard_item[1].classList.add("game-players-score");

socket.onopen = (event) => {
    socket.onmessage = (msg) => {
        // uses twemoji (https://github.com/twitter/twemoji) as alternative option on mobile
        // thanks twemoji for emoji graphics that look really nice!
        if (window.innerHeight / window.innerWidth > (4/3)) bomb_text = { "X": '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 36 36"><path fill="#CCD6DD" d="M24.187 9.657l5.658-5.654L32 6.16l-5.658 5.655z"/><circle fill="#31373D" cx="14" cy="22" r="14"/><path fill="#31373D" d="M19 11.342l5.658-5.657 5.657 5.658L24.657 17z"/><circle fill="#F18F26" cx="32" cy="4" r="4"/><circle fill="#FDCB58" cx="32" cy="4" r="2"/></svg>', "L": "<<", "R": ">>" };
        console.log(msg);
        let msg_split = msg.data.split("\n");
        let row_count = 1;
        console.log(msg_split);
        switch (msg_split[0]) {
            case "hello":
                var bomb_div = document.getElementById("game-bomb-div");
                bomb_count = parseInt(msg_split[1]);
                for (let i = 1; i < bomb_count; i++) {
                    clickable.push(false);
                    bomb_div.appendChild(bomb_div.children[0].cloneNode(true));
                    bomb_div.children[i].setAttribute("bomb_index", i);
                }
                for (let i = 0; i < bomb_count; i++) {
                    bomb_div.children[i].children[0].addEventListener("click", e => {
                        let bomb_index = e.target.parentElement.getAttribute("bomb_index");
                        if (clickable[parseInt(bomb_index)]) {
                            socket.send("move\n" + bomb_index + " L3")
                            clickable[parseInt(bomb_index)] = false;
                        }
                    });
                    bomb_div.children[i].children[1].addEventListener("click", e => {
                        let bomb_index = e.target.parentElement.getAttribute("bomb_index");
                        if (clickable[parseInt(bomb_index)]) {
                            socket.send("move\n" + bomb_index + " L1")
                            clickable[parseInt(bomb_index)] = false;
                        }
                    });
                    bomb_div.children[i].children[3].addEventListener("click", e => {
                        let bomb_index = e.target.parentElement.getAttribute("bomb_index");
                        if (clickable[parseInt(bomb_index)]) {
                            socket.send("move\n" + bomb_index + " R1")
                            clickable[parseInt(bomb_index)] = false;
                        }
                    });
                    bomb_div.children[i].children[4].addEventListener("click", e => {
                        let bomb_index = e.target.parentElement.getAttribute("bomb_index");
                        if (clickable[parseInt(bomb_index)]) {
                            socket.send("move\n" + bomb_index + " R2")
                            clickable[parseInt(bomb_index)] = false;
                        }
                    });
                }
                socket.send("olleh\n" + id);
                break;
            case "name":
                console.log("player name: " + msg_split[1]);
                document.getElementById("game-player-name").innerText = msg_split[1];
                console.log("player color: " + msg_split[2]);
                document.getElementById("game-player-color").style.backgroundColor = msg_split[2];
                break;
            case "status":
                var bomb_div = document.getElementById("game-bomb-div");
                console.log("status: " + msg_split[1]);
                bomb_div.children[msg_split[1].split(" ")[0]].children[2].innerHTML = bomb_text[msg_split[1].split(" ")[1]];
                if (msg_split[1].split(" ")[1] == "X") {
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.remove("game-bomb-display-l");
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.remove("game-bomb-display-r");
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.add("game-bomb-display-x");
                    clickable[parseInt(msg_split[1].split(" ")[0])] = true;
                } else if (msg_split[1].split(" ")[1] == "L") {
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.remove("game-bomb-display-x");
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.remove("game-bomb-display-r");
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.add("game-bomb-display-l");
                } else if (msg_split[1].split(" ")[1] == "R") {
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.remove("game-bomb-display-l");
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.remove("game-bomb-display-x");
                    bomb_div.children[msg_split[1].split(" ")[0]].children[2].classList.add("game-bomb-display-r");
                }
                break;
            case "board":
                let board_count = Math.floor((msg_split.length - 1) / 3);
                let scoreboard_div = document.getElementById("game-scoreboard");
                let new_entries = []
                for (let i = 0; i < board_count; i++) {
                    new_entries.push(scoreboard_item[0].cloneNode(true));
                    new_entries[i * 3].children[0].style.backgroundColor = msg_split[1 + 3 * i + 1];
                    new_entries[i * 3].children[1].innerText = msg_split[1 + 3 * i];
                    new_entries.push(scoreboard_item[1].cloneNode(true));
                    new_entries[i * 3 + 1].innerText = msg_split[1 + 3 * i + 2];
                    new_entries.push(scoreboard_item[1].cloneNode(true));
                }
                scoreboard_div.replaceChildren(...new_entries);
                break;
        }
    };
};
