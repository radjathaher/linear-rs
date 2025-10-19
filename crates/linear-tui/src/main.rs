use anyhow::Result;

fn main() -> Result<()> {
    linear_core::init()?;
    println!("linear-tui scaffolding ready");
    Ok(())
}
