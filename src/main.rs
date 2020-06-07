use crate::{
    app::{App, TimeFrame, UiState, UiTarget},
    stock::Stock,
};
use argh::FromArgs;
use async_std::prelude::*;
use async_std::stream;
use crossterm::{
    cursor,
    event::{self, Event, EventStream, KeyCode, KeyEvent, MouseButton, MouseEvent},
    execute, terminal,
};
use std::io::{self, Write};
use std::panic;
use std::str::FromStr;
use std::time;
use tui::{backend::CrosstermBackend, Terminal};
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
    /// debug draw
    #[argh(switch)]
    debug_draw: bool,
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
        ui_state: {
            let mut ui_state = UiState::default();
            ui_state.set_time_frame(args.time_frame)?;
            ui_state.set_debug_draw(args.debug_draw)?;
            ui_state
        },
        stock: Stock {
            bars: vec![],
            profile: None,
            quote: None,
            symbol: args.symbol,
        },
    };

    // Draw early to show an unpopulated UI before data is loaded
    terminal.draw(|mut f| {
        ui::draw(&mut f, &app).expect("Draw failed");
    })?;

    app.load_stock(&app.stock.symbol.clone()).await?;

    let input_event_stream = EventStream::new()
        .filter(|e| match e {
            Ok(Event::Key(_)) | Ok(Event::Mouse(_)) => true,
            _ => false,
        })
        .map(|maybe_event| match maybe_event {
            Ok(Event::Key(key_event)) => InputEvent::Key(key_event),
            Ok(Event::Mouse(mouse_event)) => InputEvent::Mouse(mouse_event),
            _ => unreachable!(),
        });
    let tick_stream = stream::interval(time::Duration::from_millis(TICK_RATE));
    let input_tick_stream = tick_stream.map(|()| InputEvent::Tick);
    let mut input_event_stream = input_event_stream.merge(input_tick_stream);

    loop {
        app.ui_state.clear_target_areas()?;

        terminal.draw(|mut f| {
            ui::draw(&mut f, &app).expect("Draw failed");
        })?;

        execute!(
            terminal.backend_mut(),
            cursor::Hide,
            cursor::MoveTo(0, 0),
            cursor::DisableBlinking
        )?;

        if app.ui_state.stock_symbol_input_state.active {
            let (cx, cy) = app
                .ui_state
                .input_cursor(
                    &app.ui_state.stock_symbol_input_state,
                    UiTarget::StockSymbolInput,
                )
                .unwrap();

            execute!(
                terminal.backend_mut(),
                cursor::Show,
                cursor::MoveTo(cx, cy),
                cursor::EnableBlinking
            )?;
        }

        match input_event_stream.next().await.unwrap() {
            InputEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Backspace if app.ui_state.stock_symbol_input_state.active => {
                    app.ui_state.stock_symbol_input_state.value.pop();
                }
                KeyCode::Enter if app.ui_state.stock_symbol_input_state.active => {
                    app.ui_state.stock_symbol_input_state.active = false;

                    execute!(terminal.backend_mut(), cursor::DisableBlinking)?;

                    app.load_stock(&app.ui_state.stock_symbol_input_state.value.clone())
                        .await?;
                }
                KeyCode::Enter if app.ui_state.time_frame_menu_state.active => {
                    app.ui_state
                        .set_time_frame(app.ui_state.time_frame_menu_state.selected().unwrap())?;

                    app.ui_state.time_frame_menu_state.active = false;

                    app.stock
                        .load_historical_prices(
                            app.ui_state.time_frame,
                            app.ui_state.start_date,
                            app.ui_state.end_date,
                        )
                        .await?;
                }
                KeyCode::Left
                    if !app.ui_state.stock_symbol_input_state.active
                        && !app.ui_state.time_frame_menu_state.active =>
                {
                    if let (Some(_), Some(first_bar)) =
                        (app.ui_state.time_frame.duration(), app.stock.bars.first())
                    {
                        app.ui_state.shift_date_range_before(first_bar.datetime())?;

                        app.stock
                            .load_historical_prices(
                                app.ui_state.time_frame,
                                app.ui_state.start_date,
                                app.ui_state.end_date,
                            )
                            .await?;
                    }
                }
                KeyCode::Right
                    if !app.ui_state.stock_symbol_input_state.active
                        && !app.ui_state.time_frame_menu_state.active =>
                {
                    if let (Some(_), Some(last_bar)) =
                        (app.ui_state.time_frame.duration(), app.stock.bars.last())
                    {
                        app.ui_state.shift_date_range_after(last_bar.datetime())?;

                        app.stock
                            .load_historical_prices(
                                app.ui_state.time_frame,
                                app.ui_state.start_date,
                                app.ui_state.end_date,
                            )
                            .await?;
                    }
                }
                KeyCode::Up if app.ui_state.time_frame_menu_state.active => {
                    app.ui_state.time_frame_menu_state.select_prev()?;
                }
                KeyCode::Down if app.ui_state.time_frame_menu_state.active => {
                    app.ui_state.time_frame_menu_state.select_next()?;
                }
                KeyCode::Char(c) if app.ui_state.stock_symbol_input_state.active => {
                    app.ui_state
                        .stock_symbol_input_state
                        .value
                        .push(c.to_ascii_uppercase());
                }
                KeyCode::Char(_) if app.ui_state.time_frame_menu_state.active => {}
                KeyCode::Char('q') => {
                    break;
                }
                KeyCode::Char('s') => {
                    app.ui_state.stock_symbol_input_state.active = true;
                    app.ui_state.time_frame_menu_state.active = false;
                }
                KeyCode::Char('t') => {
                    app.ui_state.time_frame_menu_state.active = true;
                    app.ui_state.stock_symbol_input_state.active = false;
                }
                KeyCode::Esc if app.ui_state.stock_symbol_input_state.active => {
                    app.ui_state.stock_symbol_input_state.active = false;
                }
                KeyCode::Esc if app.ui_state.time_frame_menu_state.active => {
                    app.ui_state.time_frame_menu_state.active = false;
                }
                _ => {}
            },
            InputEvent::Mouse(MouseEvent::Up(MouseButton::Left, col, row, _)) => {
                match app.ui_state.target_area(col, row) {
                    Some((UiTarget::StockSymbol, _)) | Some((UiTarget::StockName, _)) => {
                        app.ui_state.stock_symbol_input_state.active =
                            !app.ui_state.stock_symbol_input_state.active;
                        app.ui_state.time_frame_menu_state.active = false;
                    }
                    Some((UiTarget::TimeFrame, _)) => {
                        app.ui_state.time_frame_menu_state.active =
                            !app.ui_state.time_frame_menu_state.active;
                        app.ui_state.stock_symbol_input_state.active = false;
                    }
                    Some((UiTarget::StockSymbolInput, _)) => {}
                    Some((UiTarget::TimeFrameMenu, area)) => {
                        if let Some(n) = app.ui_state.menu_index(
                            &app.ui_state.time_frame_menu_state,
                            area,
                            col,
                            row,
                        ) {
                            app.ui_state.time_frame_menu_state.select_nth(n)?;

                            // Trigger draw to highlight the selected item
                            terminal.draw(|mut f| {
                                ui::draw(&mut f, &app).expect("Draw failed");
                            })?;

                            app.ui_state.set_time_frame(
                                app.ui_state.time_frame_menu_state.selected().unwrap(),
                            )?;

                            app.ui_state.time_frame_menu_state.active = false;

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
                        app.ui_state.stock_symbol_input_state.active = false;
                        app.ui_state.time_frame_menu_state.active = false;
                    }
                }
            }
            InputEvent::Mouse(_) => {}
            InputEvent::Tick => {}
        };

        if !app.ui_state.stock_symbol_input_state.active {
            app.ui_state.stock_symbol_input_state.value.clear();
        }

        if !app.ui_state.time_frame_menu_state.active {
            app.ui_state.time_frame_menu_state.clear_selection()?;
            app.ui_state
                .time_frame_menu_state
                .select(app.ui_state.time_frame)?;
        }
    }

    cleanup_terminal();

    Ok(())
}
