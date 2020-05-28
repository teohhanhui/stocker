use yahoo_finance::{Bar, Interval, Profile, Quote};

#[derive(Clone, Debug)]
pub struct UiState {
    pub time_frame: Interval,
}

#[derive(Debug)]
pub struct Stock {
    pub bars: Vec<Bar>,
    pub profile: Option<Profile>,
    pub quote: Option<Quote>,
    pub symbol: String,
}

pub struct App {
    pub stock: Stock,
    pub ui_state: UiState,
}
