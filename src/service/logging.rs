use std::{
    fmt::Display,
    sync::atomic::{AtomicBool, Ordering},
};

static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(enabled: bool) {
    VERBOSE.store(enabled, Ordering::Relaxed);
}

pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

pub fn info(message: impl Display) {
    println!("[rs7z] {message}");
}

pub fn detail(message: impl Display) {
    if is_verbose() {
        println!("[rs7z:verbose] {message}");
    }
}

pub fn error(message: impl Display) {
    eprintln!("[rs7z:error] {message}");
}
