use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_stream::wrappers::ReceiverStream;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;


#[tokio::main]
async fn main() {
    let url = std::env::args()
        .nth(1)
        .expect("Please provide WebSocket URL as argument");

    let (stdin_tx, stdin_rx) = tokio::sync::mpsc::channel(32);
    let (stdout_tx, stdout_rx) = tokio::sync::mpsc::channel(32);

    // Spawn stdin reader
    tokio::spawn(read_stdin(stdin_tx));
    // Spawn stdout writer
    tokio::spawn(write_stdout(stdout_rx));

    // Connect to WebSocket
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("Failed to connect to WebSocket server");
    
    let (ws_write, ws_read) = ws_stream.split();

    // Forward stdin to WebSocket
    let stdin_to_ws = ReceiverStream::new(stdin_rx)
        .map(|msg| Ok::<_, tokio_tungstenite::tungstenite::Error>(msg))
        .forward(ws_write);
    // Forward WebSocket to stdout
    let ws_to_stdout = ws_read.for_each(|message| async {
        if let Ok(msg) = message {
            stdout_tx.send(msg).await.expect("Failed to send to stdout");
        }
    });

    futures_util::future::join(stdin_to_ws, ws_to_stdout).await;
}

async fn read_stdin(tx: tokio::sync::mpsc::Sender<Message>) {
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut buffer = String::new();

    loop {
        buffer.clear();
        match stdin.read_line(&mut buffer).await {
            Ok(n) if n > 0 => {
                println!(">> Sending to WebSocket: {}", buffer.trim());
                tx.send(Message::Text(buffer.clone()))
                    .await
                    .expect("Failed to send stdin message");
            }
            _ => break,
        }
    }
}

async fn write_stdout(mut rx: tokio::sync::mpsc::Receiver<Message>) {
    let mut stdout = tokio::io::stdout();
    while let Some(message) = rx.recv().await {
        if let Message::Text(text) = message {
            println!("<< Received from WebSocket: {}", text.trim());
            stdout
                .write_all(text.as_bytes())
                .await
                .expect("Failed to write to stdout");
            stdout.flush().await.expect("Failed to flush stdout");
        }
    }
}
