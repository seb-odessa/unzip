
use lib::UnZip;

use std::env;
use std::path::Path;

fn main()-> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} <archive.zip> <name> <destination>", args[0]);
        return Ok(());
    }

    let archive = &args[1];
    let name = &args[2];
    let destination = &args[3];

    if !Path::new(archive).exists() {
        eprintln!("Каталог {archive} не существует");
        return Ok(());
    }
    if !Path::new(destination).exists() {
        eprintln!("Каталог {destination} не существует");
        return Ok(());
    }

    UnZip::try_from(archive, destination)?.file(name)
}


