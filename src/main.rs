mod startup;
mod ui;

fn main() {
    let path = std::env::current_dir()
        .map_err(|error| format!("Cannot determine the current directory: {error}"))
        .and_then(|directory| {
            startup::path(
                std::env::args_os().skip(1),
                &directory,
                std::env::home_dir().as_deref(),
            )
        });
    match path {
        Ok(path) => ui::run(path),
        Err(error) => {
            eprintln!("rs7z: {error}\nUsage: rs7z [path]");
            std::process::exit(2);
        }
    }
}
