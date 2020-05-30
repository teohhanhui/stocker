use crate::{
    app::{App, MenuState, TimeFrame, UiState},
    stock::Stock,
};
use argh::FromArgs;
use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyEvent},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::error::Error;
use std::io::{self, Write};
use std::str::FromStr;
use std::time::{Duration, Instant};
use strum::IntoEnumIterator;
use tokio::sync::mpsc;
use tui::{backend::CrosstermBackend, Terminal};

mod app;
mod stock;
mod ui;

const DEFAULT_SYMBOL: &str = "TSLA";
const DEFAULT_TIME_FRAME: &str = "1mo";
const TICK_RATE: u64 = 100;

/// Stocks dashboard
#[derive(Debug, FromArgs)]
struct Args {
    /// stock symbol
    #[argh(option, short = 's', default = "DEFAULT_SYMBOL.to_owned()")]
    symbol: String,
    /// time frame for historical prices
    #[argh(
        option,
        short = 't',
        default = "TimeFrame::from_str(DEFAULT_TIME_FRAME).unwrap()"
    )]
    time_frame: TimeFrame,
}

#[derive(Debug)]
enum InputEvent {
    Input(CrosstermEvent),
    Tick,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Args = argh::from_env();

    terminal::enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);

    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let (mut tx, mut rx) = mpsc::channel(100);

    let mut app = App {
        ui_state: UiState {
            time_frame: args.time_frame,
            time_frame_menu_state: {
                let menu_state = MenuState::new(TimeFrame::iter());
                menu_state.select(args.time_frame).unwrap();
                menu_state
            },
        },
        stock: Stock {
            bars: vec![],
            profile: None,
            quote: None,
            symbol: args.symbol,
        },
    };

    app.stock.load_profile().await?;

    app.stock
        .load_historical_prices(app.ui_state.time_frame)
        .await?;

    let tick_rate = Duration::from_millis(TICK_RATE);

    tokio::spawn(async move {
        let mut last_tick = Instant::now();

        loop {
            if event::poll(tick_rate).unwrap_or(false) {
                if let Ok(ev) = event::read() {
                    if tx.send(InputEvent::Input(ev)).await.is_err() {
                        break;
                    }
                }
            }

            if last_tick.elapsed() >= tick_rate {
                if tx.send(InputEvent::Tick).await.is_err() {
                    break;
                }
                last_tick = Instant::now();
            }
        }
    });

    terminal.clear()?;

    loop {
        match rx.recv().await.unwrap() {
            InputEvent::Input(CrosstermEvent::Key(KeyEvent { code, .. })) => match code {
                KeyCode::Enter => {
                    if let ref mut menu_state @ MenuState { active: true, .. } =
                        app.ui_state.time_frame_menu_state
                    {
                        app.ui_state.time_frame = menu_state.selected().unwrap();

                        menu_state.active = false;
                        menu_state.clear_selection();
                        menu_state.select(app.ui_state.time_frame).unwrap();

                        app.stock
                            .load_historical_prices(app.ui_state.time_frame)
                            .await?;
                    }
                }
                KeyCode::Up => {
                    if let ref mut menu_state @ MenuState { active: true, .. } =
                        app.ui_state.time_frame_menu_state
                    {
                        menu_state.select_prev();
                    }
                }
                KeyCode::Down => {
                    if let ref mut menu_state @ MenuState { active: true, .. } =
                        app.ui_state.time_frame_menu_state
                    {
                        menu_state.select_next();
                    }
                }
                KeyCode::Char('q') => {
                    break;
                }
                KeyCode::Char('t') => {
                    app.ui_state.time_frame_menu_state.active = true;
                }
                KeyCode::Esc => {
                    if let ref mut menu_state @ MenuState { active: true, .. } =
                        app.ui_state.time_frame_menu_state
                    {
                        menu_state.active = false;
                        menu_state.clear_selection();
                        menu_state.select(app.ui_state.time_frame).unwrap();
                    }
                }
                _ => {}
            },
            InputEvent::Input(_) => {}
            InputEvent::Tick => {}
        };

        terminal.draw(|mut f| ui::draw(&mut f, &app))?;
    }

    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    terminal::disable_raw_mode()?;

    Ok(())
}
