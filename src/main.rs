use crate::app::{App, Stock, UiState};
use argh::FromArgs;
use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyEvent},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::error::Error;
use std::io::{self, Write};
use std::time::{Duration as StdDuration, Instant};
use tokio::sync::mpsc;
use tui::{backend::CrosstermBackend, Terminal};
use yahoo_finance::{history, Interval, Profile};

mod app;
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
        default = "time_frame_as_interval(DEFAULT_TIME_FRAME).unwrap()",
        from_str_fn(time_frame_as_interval)
    )]
    time_frame: Interval,
}

fn time_frame_as_interval(time_frame: &str) -> Result<Interval, String> {
    Ok(match time_frame {
        "1d" => Interval::_1d,
        "5d" => Interval::_5d,
        "1mo" => Interval::_1mo,
        "3mo" => Interval::_3mo,
        "6mo" => Interval::_6mo,
        "1y" => Interval::_1y,
        "2y" => Interval::_2y,
        "5y" => Interval::_5y,
        "10y" => Interval::_10y,
        "ytd" => Interval::_ytd,
        "max" => Interval::_max,
        t => {
            return Err(format!("unrecognized time frame {}", t));
        }
    })
}

#[derive(Debug)]
enum InputEvent {
    Input(char),
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
        },
        stock: Stock {
            bars: vec![],
            profile: None,
            quote: None,
            symbol: args.symbol,
        },
    };

    app.stock.profile = Some(Profile::load(app.stock.symbol.as_str()).await?);

    app.stock.bars = history::retrieve_interval(
        app.stock.symbol.as_str(),
        time_frame_as_interval(app.ui_state.time_frame.to_string().as_str())?, // hacky hack
    )
    .await?;

    let tick_rate = StdDuration::from_millis(TICK_RATE);

    tokio::spawn(async move {
        let mut last_tick = Instant::now();

        loop {
            if event::poll(tick_rate).unwrap_or(false) {
                if let Ok(CrosstermEvent::Key(KeyEvent {
                    code: KeyCode::Char(c),
                    ..
                })) = event::read()
                {
                    if tx.send(InputEvent::Input(c)).await.is_err() {
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
            InputEvent::Input('q') => {
                break;
            }
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
