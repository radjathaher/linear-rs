use anyhow::Result;

fn main() -> Result<()> {
    linear_core::init()?;
    println!("linear-cli scaffolding ready");
    Ok(())
}
