#[tokio::main]
async fn main() -> anyhow::Result<()> {
    grate_bot::bot::run().await
}
