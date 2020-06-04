#![recursion_limit = "1024"]

use crate::{
    app::{App, InputState, MenuState, TimeFrame, UiState, UiTarget},
    stock::Stock,
};
use argh::FromArgs;
use chrono::{Duration, Utc};
use crossterm::{
    cursor,
    event::{self, Event, EventStream, KeyCode, KeyEvent, MouseButton, MouseEvent},
    execute, terminal,
};
use futures::{future::FutureExt, select, StreamExt};
use futures_timer::Delay;
use im::ordmap;
use parking_lot::RwLock;
use std::io::{self, Write};
use std::panic;
use std::str::FromStr;
use std::time;
use strum::IntoEnumIterator;
use tui::{backend::CrosstermBackend, layout::Rect, Terminal};
use yahoo_finance::Timestamped;

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

fn setup_terminal() {
    let mut stdout = io::stdout();

    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        cursor::DisableBlinking,
        event::EnableMouseCapture
    )
    .unwrap();

    // Needed for when run in a TTY since TTYs don't actually have an alternate screen.
    //
    // Must be executed after attempting to enter the alternate screen so that it only clears the
    // primary screen if we are running in a TTY.
    //
    // If not running in a TTY, then we just end up clearing the alternate screen which should have
    // no effect.
    execute!(stdout, terminal::Clear(terminal::ClearType::All)).unwrap();

    terminal::enable_raw_mode().unwrap();
}

// Adapted from https://github.com/cjbassi/ytop/blob/89a210f0e5e2de6aa0e8d7a153a21f959d77607e/src/main.rs#L51-L66
fn cleanup_terminal() {
    let mut stdout = io::stdout();

    // Needed for when run in a TTY since TTYs don't actually have an alternate screen.
    //
    // Must be executed before attempting to leave the alternate screen so that it only modifies the
    // primary screen if we are running in a TTY.
    //
    // If not running in a TTY, then we just end up modifying the alternate screen which should have
    // no effect.
    execute!(
        stdout,
        cursor::MoveTo(0, 0),
        terminal::Clear(terminal::ClearType::All)
    )
    .unwrap();

    execute!(
        stdout,
        terminal::LeaveAlternateScreen,
        cursor::Show,
        cursor::EnableBlinking,
        event::DisableMouseCapture
    )
    .unwrap();

    terminal::disable_raw_mode().unwrap();
}

// Adapted from https://github.com/cjbassi/ytop/blob/89a210f0e5e2de6aa0e8d7a153a21f959d77607e/src/main.rs#L113-L120
//
// We need to catch panics since we need to close the UI and cleanup the terminal before logging any
// error messages to the screen.
fn setup_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        cleanup_terminal();
        better_panic::Settings::auto().create_panic_handler()(panic_info);
    }));
}

#[smol_potat::main]
async fn main() -> anyhow::Result<()> {
    better_panic::install();

    let args: Args = argh::from_env();

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    setup_panic_hook();
    setup_terminal();

    let mut app = App {
        ui_state: UiState {
            end_date: None,
            start_date: None,
            stock_symbol_input_state: InputState::default(),
            target_areas: RwLock::new(ordmap! {}),
            time_frame: args.time_frame,
            time_frame_menu_state: {
                let menu_state = MenuState::new(TimeFrame::iter());
                menu_state.select(args.time_frame)?;
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
        .load_historical_prices(
            app.ui_state.time_frame,
            app.ui_state.start_date,
            app.ui_state.end_date,
        )
        .await?;

    let mut reader = EventStream::new();

    loop {
        let UiState {
            ref target_areas, ..
        } = app.ui_state;

        target_areas.write().clear();

        terminal.draw(|mut f| {
            ui::draw(&mut f, &app).expect("Draw failed");
        })?;

        execute!(
            terminal.backend_mut(),
            cursor::Hide,
            cursor::DisableBlinking
        )?;

        let stock_symbol_input_state = &mut app.ui_state.stock_symbol_input_state;
        let time_frame_menu_state = &mut app.ui_state.time_frame_menu_state;

        if stock_symbol_input_state.active {
            let Rect { x, y, .. } = *target_areas
                .read()
                .get(&UiTarget::StockSymbolInput)
                .unwrap();
            let cx = x + 1 + stock_symbol_input_state.value.chars().count() as u16;
            let cy = y + 1;

            execute!(
                terminal.backend_mut(),
                cursor::Show,
                cursor::MoveTo(cx, cy),
                cursor::EnableBlinking
            )?;
        }

        let mut delay = Delay::new(time::Duration::from_millis(TICK_RATE)).fuse();
        let mut event = reader.next().fuse();

        select! {
            maybe_event = event => match maybe_event {
                Some(Ok(event)) => match event {
                    Event::Key(KeyEvent { code, .. }) => match code {
                        KeyCode::Backspace if stock_symbol_input_state.active => {
                            stock_symbol_input_state.value.pop();
                        }
                        KeyCode::Enter if stock_symbol_input_state.active => {
                            app.stock.symbol = stock_symbol_input_state.value.to_ascii_uppercase();
                            app.ui_state.start_date = None;
                            app.ui_state.end_date = None;

                            stock_symbol_input_state.active = false;

                            execute!(terminal.backend_mut(), cursor::DisableBlinking)?;

                            app.stock.load_profile().await?;
                            app.stock
                                .load_historical_prices(
                                    app.ui_state.time_frame,
                                    app.ui_state.start_date,
                                    app.ui_state.end_date,
                                )
                                .await?;
                        }
                        KeyCode::Enter if time_frame_menu_state.active => {
                            app.ui_state.time_frame = time_frame_menu_state.selected().unwrap();
                            app.ui_state.start_date = None;
                            app.ui_state.end_date = None;

                            time_frame_menu_state.active = false;

                            app.stock
                                .load_historical_prices(
                                    app.ui_state.time_frame,
                                    app.ui_state.start_date,
                                    app.ui_state.end_date,
                                )
                                .await?;
                        }
                        KeyCode::Left => {
                            if let Some(duration) = app.ui_state.time_frame.duration() {
                                app.ui_state.end_date = if let Some(first_bar) = app.stock.bars.first() {
                                    Some(
                                        (first_bar.datetime() - Duration::days(1))
                                            .date()
                                            .and_hms(23, 59, 59),
                                    )
                                } else {
                                    None
                                };
                                app.ui_state.start_date = if let Some(end_date) = app.ui_state.end_date {
                                    Some(
                                        (end_date - duration + Duration::days(1))
                                            .date()
                                            .and_hms(0, 0, 0),
                                    )
                                } else {
                                    None
                                };

                                app.stock
                                    .load_historical_prices(
                                        app.ui_state.time_frame,
                                        app.ui_state.start_date,
                                        app.ui_state.end_date,
                                    )
                                    .await?;
                            }
                        }
                        KeyCode::Right => {
                            if let Some(duration) = app.ui_state.time_frame.duration() {
                                app.ui_state.start_date = if let Some(last_bar) = app.stock.bars.last() {
                                    Some(
                                        (last_bar.datetime() + Duration::days(1))
                                            .date()
                                            .and_hms(0, 0, 0),
                                    )
                                } else {
                                    None
                                };
                                app.ui_state.end_date = if let Some(start_date) = app.ui_state.start_date {
                                    Some(
                                        (start_date + duration - Duration::days(1))
                                            .date()
                                            .and_hms(23, 59, 59),
                                    )
                                } else {
                                    None
                                };

                                if let Some(end_date) = app.ui_state.end_date {
                                    let now = Utc::now();
                                    if end_date > now {
                                        app.ui_state.start_date = None;
                                        app.ui_state.end_date = None;
                                    }
                                }

                                app.stock
                                    .load_historical_prices(
                                        app.ui_state.time_frame,
                                        app.ui_state.start_date,
                                        app.ui_state.end_date,
                                    )
                                    .await?;
                            }
                        }
                        KeyCode::Up if time_frame_menu_state.active => {
                            time_frame_menu_state.select_prev()?;
                        }
                        KeyCode::Down if time_frame_menu_state.active => {
                            time_frame_menu_state.select_next()?;
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
                    }
                    Event::Mouse(MouseEvent::Up(MouseButton::Left, col, row, _)) => {
                        match target_areas.read().iter().rev().find(|(_, area)| {
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
                                    app.ui_state.start_date = None;
                                    app.ui_state.end_date = None;

                                    time_frame_menu_state.active = false;

                                    app.stock
                                        .load_historical_prices(
                                            app.ui_state.time_frame,
                                            app.ui_state.start_date,
                                            app.ui_state.end_date,
                                        )
                                        .await?;
                                }
                            }
                            None => {
                                stock_symbol_input_state.active = false;
                                time_frame_menu_state.active = false;
                            }
                        }
                    },
                    Event::Mouse(_) => {}
                    _ => {}
                }
                Some(Err(e)) => {
                    panic!("Error: {:?}", e);
                }
                None => {}
            },
            _ = delay => {}
        };

        if !stock_symbol_input_state.active {
            stock_symbol_input_state.value.clear();
        }

        if !time_frame_menu_state.active {
            time_frame_menu_state.clear_selection();
            time_frame_menu_state.select(app.ui_state.time_frame)?;
        }
    }

    cleanup_terminal();

    Ok(())
}
