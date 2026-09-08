//! A full stderr pipe must not stop streamed stdout from reaching the app.
use super::*;

#[tokio::test]
async fn stderr_larger_than_the_pipe_is_drained_while_reading_the_answer() {
    let cli = fake_cli(
        "stderr-backpressure",
        r#"cat > /dev/null
head -c 262144 /dev/zero >&2
echo '{"type":"result","is_error":false,"result":"Take Bowers.","usage":{"input_tokens":7,"output_tokens":2},"modelUsage":{"claude-opus-5":{}}}'"#,
    );
    let reply = ask_within(
        &cli,
        ChatModel::Opus5,
        Effort::High,
        "the board",
        &question(),
        CancelSignal::never(),
        Duration::from_secs(3),
        None,
    )
    .await;
    remove(&cli);
    assert_eq!(
        reply.expect("stderr must not block stdout").text,
        "Take Bowers."
    );
}
