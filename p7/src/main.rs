use std::{error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let file_path = Path::new("tests/test.p7");
    let contents = fs::read_to_string(file_path)?;

    let result = p7::compile_and_run(contents, "test")?;
    println!("Result: {:?}", result);

    Ok(())
}
