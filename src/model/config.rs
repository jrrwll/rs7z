#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalApp {
    Terminal,
    ITerm2,
    Warp,
    Alacritty,
    Kitty,
    WezTerm,
    Ghostty,
    Gnome,
    Kde,
    Xfce4,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalConfig {
    Disabled,
    App(TerminalApp),
    Command(Vec<String>),
}
