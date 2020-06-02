use crate::{
    app::{App, InputState, MenuState, TimeFrame, UiState, UiTarget},
    stock::Stock,
};
use argh::FromArgs;
use chrono::Utc;
use crossterm::{
    cursor::{DisableBlinking, EnableBlinking},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent, MouseButton, MouseEvent,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use im::ordmap;
use parking_lot::RwLock;
use std::error::Error;
use std::io::{self, Write};
use std::str::FromStr;
use std::time::{Duration, Instant};
use strum::IntoEnumIterator;
use tokio::sync::mpsc;
use tui::{backend::CrosstermBackend, layout::Rect, Terminal};

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
    Key(KeyEvent),
    Mouse(MouseEvent),
    Tick,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Args = argh::from_env();

    terminal::enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        DisableBlinking
    )?;

    let backend = CrosstermBackend::new(stdout);

    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let (mut tx, mut rx) = mpsc::channel(100);

    let mut app = App {
        ui_state: UiState {
            end_date: Utc::now(),
            stock_symbol_input_state: InputState::default(),
            target_areas: RwLock::new(ordmap! {}),
            time_frame: args.time_frame,
            time_frame_menu_state: {
                let menu_state = MenuState::new(TimeFrame::iter());
                menu_state.select(args.time_frame);
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
        .load_historical_prices(app.ui_state.time_frame, app.ui_state.end_date)
        .await?;

    let tick_rate = Duration::from_millis(TICK_RATE);

    tokio::spawn(async move {
        let mut last_tick = Instant::now();

        loop {
            if last_tick.elapsed() >= tick_rate {
                if tx.send(InputEvent::Tick).await.is_err() {
                    break;
                }
                last_tick = Instant::now();
            }

            if event::poll(tick_rate).unwrap() {
                let input_event = match event::read().unwrap() {
                    event::Event::Key(key_event) => InputEvent::Key(key_event),
                    event::Event::Mouse(mouse_event) => InputEvent::Mouse(mouse_event),
                    _ => {
                        continue;
                    }
                };

                if tx.send(input_event).await.is_err() {
                    break;
                }
            }
        }
    });

    terminal.clear()?;

    loop {
        let UiState {
            ref target_areas, ..
        } = app.ui_state;

        target_areas.write().clear();

        terminal.draw(|mut f| ui::draw(&mut f, &app))?;

        terminal.hide_cursor()?;
        execute!(terminal.backend_mut(), DisableBlinking)?;

        let stock_symbol_input_state = &mut app.ui_state.stock_symbol_input_state;
        let time_frame_menu_state = &mut app.ui_state.time_frame_menu_state;

        if stock_symbol_input_state.active {
            terminal.show_cursor()?;

            let Rect { x, y, .. } = *target_areas
                .read()
                .get(&UiTarget::StockSymbolInput)
                .unwrap();
            terminal.set_cursor(
                x + 1 + stock_symbol_input_state.value.chars().count() as u16,
                y + 1,
            )?;
            execute!(terminal.backend_mut(), EnableBlinking)?;
        }

        match rx.recv().await.unwrap() {
            InputEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Backspace if stock_symbol_input_state.active => {
                    stock_symbol_input_state.value.pop();
                }
                KeyCode::Enter if stock_symbol_input_state.active => {
                    app.stock.symbol = stock_symbol_input_state.value.to_ascii_uppercase();

                    stock_symbol_input_state.active = false;

                    execute!(terminal.backend_mut(), DisableBlinking)?;

                    app.stock.load_profile().await?;
                    app.stock
                        .load_historical_prices(app.ui_state.time_frame, app.ui_state.end_date)
                        .await?;
                }
                KeyCode::Enter if time_frame_menu_state.active => {
                    app.ui_state.time_frame = time_frame_menu_state.selected().unwrap();

                    time_frame_menu_state.active = false;

                    app.stock
                        .load_historical_prices(app.ui_state.time_frame, app.ui_state.end_date)
                        .await?;
                }
                KeyCode::Left => {
                    // TODO: Stop when data is empty?
                    if let Some(duration) = app.ui_state.time_frame.duration() {
                        app.ui_state.end_date = app.ui_state.end_date - duration;

                        app.stock
                            .load_historical_prices(app.ui_state.time_frame, app.ui_state.end_date)
                            .await?;
                    }
                }
                KeyCode::Right => {
                    if let Some(duration) = app.ui_state.time_frame.duration() {
                        app.ui_state.end_date = app.ui_state.end_date + duration;
                    }

                    let now = Utc::now();
                    if app.ui_state.end_date > now {
                        app.ui_state.end_date = now;
                    }

                    app.stock
                        .load_historical_prices(app.ui_state.time_frame, app.ui_state.end_date)
                        .await?;
                }
                KeyCode::Up if time_frame_menu_state.active => {
                    time_frame_menu_state.select_prev();
                }
                KeyCode::Down if time_frame_menu_state.active => {
                    time_frame_menu_state.select_next();
                }
                KeyCode::Char(c) if stock_symbol_input_state.active => {
                    stock_symbol_input_state.value.push(c.to_ascii_uppercase());
                }
                KeyCode::Char('q') => {
                    break;
                }
                KeyCode::Char('s') => {
                    stock_symbol_input_state.active = true;
                    time_frame_menu_state.active = false;
                }
                KeyCode::Char('t') => {
                    time_frame_menu_state.active = true;
                    stock_symbol_input_state.active = false;
                }
                KeyCode::Esc if stock_symbol_input_state.active => {
                    stock_symbol_input_state.active = false;
                }
                KeyCode::Esc if time_frame_menu_state.active => {
                    time_frame_menu_state.active = false;
                }
                _ => {}
            },
            InputEvent::Mouse(MouseEvent::Up(MouseButton::Left, col, row, _)) => match target_areas
                .read()
                .iter()
                .rev()
                .find(|(_, area)| {
                    area.left() <= col
                        && area.right() >= col
                        && area.top() <= row
                        && area.bottom() >= row
                }) {
                Some((UiTarget::StockSymbol, _)) | Some((UiTarget::StockName, _)) => {
                    stock_symbol_input_state.active = !stock_symbol_input_state.active;
                    time_frame_menu_state.active = false;
                }
                Some((UiTarget::TimeFrame, _)) => {
                    time_frame_menu_state.active = !time_frame_menu_state.active;
                    stock_symbol_input_state.active = false;
                }
                Some((UiTarget::StockSymbolInput, _)) => {}
                Some((UiTarget::TimeFrameMenu, area)) => {
                    if area.height as usize - 2 < time_frame_menu_state.items.len() {
                        todo!("not sure how to select an item from scrollable list");
                    }
                    let n: usize = (row - area.top() - 1) as usize;

                    if n < time_frame_menu_state.items.len() {
                        time_frame_menu_state.select_nth(n);

                        app.ui_state.time_frame = time_frame_menu_state.selected().unwrap();

                        time_frame_menu_state.active = false;

                        app.stock
                            .load_historical_prices(app.ui_state.time_frame, app.ui_state.end_date)
                            .await?;
                    }
                }
                None => {
                    stock_symbol_input_state.active = false;
                    time_frame_menu_state.active = false;
                }
            },
            InputEvent::Mouse(_) => {}
            InputEvent::Tick => {}
        };

        if !stock_symbol_input_state.active {
            stock_symbol_input_state.value.clear();
        }

        if !time_frame_menu_state.active {
            time_frame_menu_state.clear_selection();
            time_frame_menu_state.select(app.ui_state.time_frame);
        }
    }

    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        EnableBlinking,
    )?;
    terminal.show_cursor()?;

    terminal::disable_raw_mode()?;

    Ok(())
}
