//! A language server the integration tests script. It speaks JSON-RPC on
//! stdio and does only what its script says. `test_support` starts it.
//!
//! The first argument is the script, a JSON object:
//!
//! ```json
//! {
//!   "capabilities": { "definitionProvider": true },
//!   "answers": {
//!     "textDocument/definition": {
//!       "after": "textDocument/didChange",
//!       "ask": { "method": "workspace/configuration", "params": {}, "expect": [] },
//!       "result": { "uri": "file:///tmp/a.fake", "range": { ... } }
//!     }
//!   }
//! }
//! ```
//!
//! `initialize` gets the capabilities. A request in `answers` gets its
//! `result`, and any other request gets `null`. Before it answers, the
//! server waits for the `after` notification to arrive, then sends the `ask`
//! request and waits for the editor's answer. If that answer isn't `expect`,
//! the request gets an error that says what came instead. Both are optional.
//! `exit`, or the end of stdin, ends the server.

use std::{
    collections::VecDeque,
    io::{self, BufRead, Read, Write},
    process::exit,
};

use serde_json::{json, Value};

fn main() {
    let script = std::env::args()
        .nth(1)
        .expect("the script is the first argument");
    let script: Value = serde_json::from_str(&script).expect("the script is JSON");
    let mut server = Server {
        input: io::stdin().lock(),
        queue: VecDeque::new(),
        asked: 0,
    };

    loop {
        let message = server.next();
        let Some(method) = message["method"].as_str() else {
            continue;
        };
        let id = &message["id"];
        if id.is_null() {
            if method == "exit" {
                exit(0);
            }
            continue;
        }
        let mut reply = match method {
            "initialize" => json!({ "result": { "capabilities": script["capabilities"] } }),
            _ => match script["answers"].get(method) {
                Some(answer) => server.answer(answer),
                None => json!({ "result": null }),
            },
        };
        reply["jsonrpc"] = json!("2.0");
        reply["id"] = id.clone();
        send(&reply);
    }
}

struct Server {
    input: io::StdinLock<'static>,
    /// Messages read while waiting for another, not handled yet.
    queue: VecDeque<Value>,
    asked: u64,
}

impl Server {
    fn next(&mut self) -> Value {
        self.queue.pop_front().unwrap_or_else(|| self.read())
    }

    fn answer(&mut self, answer: &Value) -> Value {
        if let Some(method) = answer["after"].as_str() {
            self.wait_for(|message| message["method"] == method && message["id"].is_null());
        }
        if let Some(ask) = answer.get("ask") {
            self.asked += 1;
            let id = format!("fake-lsp-{}", self.asked);
            send(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": ask["method"],
                "params": ask["params"],
            }));
            let response = self.wait_for(|message| message["id"] == id);
            let got = response.get("result").unwrap_or(&response["error"]);
            if let Some(expect) = ask.get("expect").filter(|expect| *expect != got) {
                return json!({ "error": {
                    "code": -32603,
                    "message": format!("{} answered {got}, not {expect}", ask["method"]),
                }});
            }
        }
        json!({ "result": answer["result"] })
    }

    /// Reads until a message matches, and queues the others.
    fn wait_for(&mut self, matches: impl Fn(&Value) -> bool) -> Value {
        loop {
            let message = self.read();
            if matches(&message) {
                return message;
            }
            self.queue.push_back(message);
        }
    }

    fn read(&mut self) -> Value {
        let mut length = None;
        loop {
            let mut line = String::new();
            if self.input.read_line(&mut line).expect("stdin reads") == 0 {
                exit(0);
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = Some(value.parse().expect("Content-Length is a number"));
            }
        }
        let mut content = vec![0; length.expect("a message has a Content-Length")];
        self.input.read_exact(&mut content).expect("stdin reads");
        serde_json::from_slice(&content).expect("a message is JSON")
    }
}

fn send(message: &Value) {
    let content = message.to_string();
    let mut stdout = io::stdout().lock();
    write!(stdout, "Content-Length: {}\r\n\r\n{content}", content.len()).expect("stdout writes");
    stdout.flush().expect("stdout writes");
}
