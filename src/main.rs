use crate::{
    app::{App, Indicator, InputState, TimeFrame, UiState, UiTarget},
    event::{InputEvent, TextFieldEvent},
    reactive::StreamExt as ReactiveStreamExt,
    stock::Stock,
};
use argh::FromArgs;
use async_std::{
    stream::{self, StreamExt},
    task,
};
use crossterm::{
    cursor,
    event::{Event, EventStream, KeyCode, KeyEvent, MouseButton, MouseEvent},
    execute, terminal,
};
use reactive_rs::{SimpleBroadcast, Stream};
use std::{
    io::{self, Write},
    panic,
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
    time,
};
use tui::{backend::CrosstermBackend, Terminal};
use yahoo_finance::Timestamped;

mod app;
mod event;
mod reactive;
mod stock;
mod ui;
mod widgets;

const DEFAULT_SYMBOL: &str = "TSLA";
const TICK_RATE: u64 = 100;

/// Stocks dashboard
#[derive(Debug, FromArgs)]
struct Args {
    /// debug draw
    #[argh(switch)]
    debug_draw: bool,
    /// indicator for technical analysis
    #[argh(option, short = 'i')]
    indicator: Option<Indicator>,
    /// stock symbol
    #[argh(option, short = 's', default = "DEFAULT_SYMBOL.to_owned()")]
    symbol: String,
    /// time frame for historical prices
    #[argh(option, short = 't', default = "TimeFrame::default()")]
    time_frame: TimeFrame,
}

fn setup_terminal() {
    let mut stdout = io::stdout();

    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        cursor::DisableBlinking,
        crossterm::event::EnableMouseCapture
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
        crossterm::event::DisableMouseCapture
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

    let should_quit = AtomicBool::new(false);

    let events = SimpleBroadcast::new()
        .start_with(InputEvent::Tick)
        .broadcast();

    let stock_symbol_text_field_events =
        event::input_events_to_text_field_events(events.clone(), KeyCode::Char('s'), |v| {
            v.to_ascii_uppercase()
        })
        .broadcast();

    let stock_symbols = stock_symbol_text_field_events
        .clone()
        .fold(args.symbol.clone(), |acc_symbol, ev| {
            if let TextFieldEvent::Accept(symbol) = ev {
                symbol.clone()
            } else {
                acc_symbol.clone()
            }
        })
        .start_with(args.symbol.clone())
        .broadcast();

    let stock_symbol_input_states =
        event::text_field_events_to_input_states(stock_symbol_text_field_events.clone())
            .broadcast();

    events
        .clone()
        .with_latest_from(
            stock_symbol_input_states.clone(),
            |(ev, stock_symbol_input_state)| (*ev, stock_symbol_input_state.clone()),
        )
        .with_latest_from(
            stock_symbols.clone(),
            |((ev, stock_symbol_input_state), stock_symbol)| {
                (*ev, stock_symbol_input_state.clone(), stock_symbol.clone())
            },
        )
        .subscribe(|(ev, stock_symbol_input_state, stock_symbol)| match ev {
            InputEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Char('q') if !stock_symbol_input_state.active => {
                    should_quit.store(true, AtomicOrdering::Relaxed);
                }
                KeyCode::Char(_) if !stock_symbol_input_state.active => {
                    execute!(terminal.backend_mut(), crossterm::style::Print("\x07"),).unwrap();
                }
                _ => {}
            },
            InputEvent::Tick => {
                let app = App {
                    stock: Stock {
                        bars: vec![],
                        profile: None,
                        quote: None,
                        symbol: stock_symbol.clone(),
                    },
                    ui_state: UiState {
                        stock_symbol_input_state: stock_symbol_input_state.clone(),
                    },
                };

                terminal
                    .draw(|mut f| {
                        ui::draw(&mut f, &app).expect("Draw failed");
                    })
                    .unwrap();
            }
            _ => {}
        });

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

    // hacky hacks
    stock_symbols.send(args.symbol);
    stock_symbol_input_states.send(InputState::default());
    events.send(InputEvent::Tick);

    while !should_quit.load(AtomicOrdering::Relaxed) {
        events.send(input_event_stream.next().await.unwrap());
    }

    // let mut app = App {
    //     ui_state: {
    //         let mut ui_state = UiState::default();
    //         ui_state.set_time_frame(args.time_frame)?;
    //         if let Some(indicator) = args.indicator {
    //             ui_state.set_indicator(indicator)?;
    //         }
    //         ui_state.set_debug_draw(args.debug_draw)?;
    //         ui_state
    //     },
    //     stock: Stock {
    //         bars: vec![],
    //         profile: None,
    //         quote: None,
    //         symbol: args.symbol,
    //     },
    // };

    // // Draw early to show an unpopulated UI before data is loaded
    // terminal.draw(|mut f| {
    //     ui::draw(&mut f, &app).expect("Draw failed");
    // })?;

    // app.load_stock(&app.stock.symbol.clone()).await?;

    // loop {
    //     match event_stream.next().await {
    //         InputEvent::Key(KeyEvent { code, .. }) => match code {
    //             KeyCode::Backspace if app.ui_state.stock_symbol_input_state.active => {
    //                 app.ui_state.stock_symbol_input_state.value.pop();
    //             }
    //             KeyCode::Enter if app.ui_state.indicator_menu_state.read().active => {
    //                 if let Some(selected_indicator) = {
    //                     let indicator_menu_state = app.ui_state.indicator_menu_state.read();
    //                     indicator_menu_state.selected()
    //                 } {
    //                     app.ui_state.set_indicator(selected_indicator)?;
    //                 } else {
    //                     app.ui_state.clear_indicator()?;
    //                 }

    //                 app.ui_state.indicator_menu_state.write().active = false;

    //                 // TODO: load the correct date range based on indicator period
    //                 // app.stock
    //                 //     .load_historical_prices(
    //                 //         app.ui_state.time_frame,
    //                 //         app.ui_state.start_date,
    //                 //         app.ui_state.end_date,
    //                 //     )
    //                 //     .await?;
    //             }
    //             KeyCode::Enter if app.ui_state.stock_symbol_input_state.active => {
    //                 app.ui_state.stock_symbol_input_state.active = false;

    //                 execute!(terminal.backend_mut(), cursor::DisableBlinking)?;

    //                 app.load_stock(&app.ui_state.stock_symbol_input_state.value.clone())
    //                     .await?;
    //             }
    //             KeyCode::Enter if app.ui_state.time_frame_menu_state.read().active => {
    //                 let selected_time_frame = {
    //                     let time_frame_menu_state = app.ui_state.time_frame_menu_state.read();
    //                     time_frame_menu_state.selected().unwrap()
    //                 };
    //                 app.ui_state.set_time_frame(selected_time_frame)?;

    //                 app.ui_state.time_frame_menu_state.write().active = false;

    //                 app.stock
    //                     .load_historical_prices(
    //                         app.ui_state.time_frame,
    //                         app.ui_state.start_date,
    //                         app.ui_state.end_date,
    //                     )
    //                     .await?;
    //             }
    //             KeyCode::Left
    //                 if !app.ui_state.stock_symbol_input_state.active
    //                     && !app.ui_state.time_frame_menu_state.read().active =>
    //             {
    //                 if let (Some(_), Some(first_bar)) =
    //                     (app.ui_state.time_frame.duration(), app.stock.bars.first())
    //                 {
    //                     app.ui_state.shift_date_range_before(first_bar.datetime())?;

    //                     app.stock
    //                         .load_historical_prices(
    //                             app.ui_state.time_frame,
    //                             app.ui_state.start_date,
    //                             app.ui_state.end_date,
    //                         )
    //                         .await?;
    //                 }
    //             }
    //             KeyCode::Right
    //                 if !app.ui_state.stock_symbol_input_state.active
    //                     && !app.ui_state.time_frame_menu_state.read().active =>
    //             {
    //                 if let (Some(_), Some(last_bar)) =
    //                     (app.ui_state.time_frame.duration(), app.stock.bars.last())
    //                 {
    //                     app.ui_state.shift_date_range_after(last_bar.datetime())?;

    //                     app.stock
    //                         .load_historical_prices(
    //                             app.ui_state.time_frame,
    //                             app.ui_state.start_date,
    //                             app.ui_state.end_date,
    //                         )
    //                         .await?;
    //                 }
    //             }
    //             KeyCode::Up if app.ui_state.indicator_menu_state.read().active => {
    //                 app.ui_state.indicator_menu_state.write().select_prev()?;
    //             }
    //             KeyCode::Up if app.ui_state.time_frame_menu_state.read().active => {
    //                 app.ui_state.time_frame_menu_state.write().select_prev()?;
    //             }
    //             KeyCode::Down if app.ui_state.indicator_menu_state.read().active => {
    //                 app.ui_state.indicator_menu_state.write().select_next()?;
    //             }
    //             KeyCode::Down if app.ui_state.time_frame_menu_state.read().active => {
    //                 app.ui_state.time_frame_menu_state.write().select_next()?;
    //             }
    //             KeyCode::Char(_) if app.ui_state.indicator_menu_state.read().active => {}
    //             KeyCode::Char(c) if app.ui_state.stock_symbol_input_state.active => {
    //                 app.ui_state
    //                     .stock_symbol_input_state
    //                     .value
    //                     .push(c.to_ascii_uppercase());
    //             }
    //             KeyCode::Char(_) if app.ui_state.time_frame_menu_state.read().active => {}
    //             KeyCode::Char('i') => {
    //                 app.ui_state.indicator_menu_state.write().active = true;
    //                 app.ui_state.stock_symbol_input_state.active = false;
    //                 app.ui_state.time_frame_menu_state.write().active = false;
    //             }
    //             KeyCode::Char('q') => {
    //                 break;
    //             }
    //             KeyCode::Char('s') => {
    //                 app.ui_state.stock_symbol_input_state.active = true;
    //                 app.ui_state.indicator_menu_state.write().active = false;
    //                 app.ui_state.time_frame_menu_state.write().active = false;
    //             }
    //             KeyCode::Char('t') => {
    //                 app.ui_state.time_frame_menu_state.write().active = true;
    //                 app.ui_state.stock_symbol_input_state.active = false;
    //                 app.ui_state.indicator_menu_state.write().active = false;
    //             }
    //             KeyCode::Esc if app.ui_state.indicator_menu_state.read().active => {
    //                 app.ui_state.indicator_menu_state.write().active = false;
    //             }
    //             KeyCode::Esc if app.ui_state.stock_symbol_input_state.active => {
    //                 app.ui_state.stock_symbol_input_state.active = false;
    //             }
    //             KeyCode::Esc if app.ui_state.time_frame_menu_state.read().active => {
    //                 app.ui_state.time_frame_menu_state.write().active = false;
    //             }
    //             _ => {}
    //         },
    //         InputEvent::Mouse(MouseEvent::Up(MouseButton::Left, col, row, _)) => {
    //             match app.ui_state.target_area(col, row) {
    //                 Some((UiTarget::IndicatorBox, _)) => {
    //                     app.ui_state.indicator_menu_state.write().active = !{
    //                         let indicator_menu_state = app.ui_state.indicator_menu_state.read();
    //                         indicator_menu_state.active
    //                     };
    //                     app.ui_state.stock_symbol_input_state.active = false;
    //                     app.ui_state.time_frame_menu_state.write().active = false;
    //                 }
    //                 Some((UiTarget::StockSymbol, _)) | Some((UiTarget::StockName, _)) => {
    //                     app.ui_state.stock_symbol_input_state.active =
    //                         !app.ui_state.stock_symbol_input_state.active;
    //                     app.ui_state.indicator_menu_state.write().active = false;
    //                     app.ui_state.time_frame_menu_state.write().active = false;
    //                 }
    //                 Some((UiTarget::TimeFrameBox, _)) => {
    //                     app.ui_state.time_frame_menu_state.write().active = !{
    //                         let time_frame_menu_state = app.ui_state.time_frame_menu_state.read();
    //                         time_frame_menu_state.active
    //                     };
    //                     app.ui_state.indicator_menu_state.write().active = false;
    //                     app.ui_state.stock_symbol_input_state.active = false;
    //                 }
    //                 Some((UiTarget::IndicatorList, area)) => {
    //                     if let Some(n) = {
    //                         let indicator_menu_state = app.ui_state.indicator_menu_state.read();
    //                         app.ui_state
    //                             .menu_index(&*indicator_menu_state, area, col, row)
    //                     } {
    //                         app.ui_state.indicator_menu_state.write().select_nth(n)?;

    //                         // Trigger draw to highlight the selected item
    //                         terminal.draw(|mut f| {
    //                             ui::draw(&mut f, &app).expect("Draw failed");
    //                         })?;

    //                         if let Some(selected_indicator) = {
    //                             let indicator_menu_state = app.ui_state.indicator_menu_state.read();
    //                             indicator_menu_state.selected()
    //                         } {
    //                             app.ui_state.set_indicator(selected_indicator)?;
    //                         } else {
    //                             app.ui_state.clear_indicator()?;
    //                         }

    //                         app.ui_state.indicator_menu_state.write().active = false;

    //                         // TODO: load the correct date range based on indicator period
    //                         // app.stock
    //                         //     .load_historical_prices(
    //                         //         app.ui_state.time_frame,
    //                         //         app.ui_state.start_date,
    //                         //         app.ui_state.end_date,
    //                         //     )
    //                         //     .await?;
    //                     }
    //                 }
    //                 Some((UiTarget::StockSymbolInput, _)) => {}
    //                 Some((UiTarget::TimeFrameList, area)) => {
    //                     if let Some(n) = {
    //                         let time_frame_menu_state = app.ui_state.time_frame_menu_state.read();
    //                         app.ui_state
    //                             .menu_index(&*time_frame_menu_state, area, col, row)
    //                     } {
    //                         app.ui_state.time_frame_menu_state.write().select_nth(n)?;

    //                         // Trigger draw to highlight the selected item
    //                         terminal.draw(|mut f| {
    //                             ui::draw(&mut f, &app).expect("Draw failed");
    //                         })?;

    //                         let selected_time_frame = {
    //                             let time_frame_menu_state =
    //                                 app.ui_state.time_frame_menu_state.read();
    //                             time_frame_menu_state.selected().unwrap()
    //                         };
    //                         app.ui_state.set_time_frame(selected_time_frame)?;

    //                         app.ui_state.time_frame_menu_state.write().active = false;

    //                         app.stock
    //                             .load_historical_prices(
    //                                 app.ui_state.time_frame,
    //                                 app.ui_state.start_date,
    //                                 app.ui_state.end_date,
    //                             )
    //                             .await?;
    //                     }
    //                 }
    //                 None => {
    //                     app.ui_state.indicator_menu_state.write().active = false;
    //                     app.ui_state.stock_symbol_input_state.active = false;
    //                     app.ui_state.time_frame_menu_state.write().active = false;
    //                 }
    //             }
    //         }
    //         InputEvent::Mouse(_) => {}
    //         InputEvent::Tick => {
    //             app.ui_state.clear_target_areas()?;

    //             terminal.draw(|mut f| {
    //                 ui::draw(&mut f, &app).expect("Draw failed");
    //             })?;

    //             execute!(
    //                 terminal.backend_mut(),
    //                 cursor::Hide,
    //                 cursor::MoveTo(0, 0),
    //                 cursor::DisableBlinking
    //             )?;

    //             if app.ui_state.stock_symbol_input_state.active {
    //                 let (cx, cy) = app
    //                     .ui_state
    //                     .input_cursor(
    //                         &app.ui_state.stock_symbol_input_state,
    //                         UiTarget::StockSymbolInput,
    //                     )
    //                     .unwrap();

    //                 execute!(
    //                     terminal.backend_mut(),
    //                     cursor::Show,
    //                     cursor::MoveTo(cx, cy),
    //                     cursor::EnableBlinking
    //                 )?;
    //             }
    //         }
    //     };

    //     if !app.ui_state.indicator_menu_state.read().active {
    //         let mut indicator_menu_state = app.ui_state.indicator_menu_state.write();
    //         indicator_menu_state.clear_selection()?;
    //         if let Some(indicator) = app.ui_state.indicator {
    //             indicator_menu_state.select(indicator).ok();
    //         } else {
    //             indicator_menu_state.select_nth(0)?;
    //         }
    //     }

    //     if !app.ui_state.stock_symbol_input_state.active {
    //         app.ui_state.stock_symbol_input_state.value.clear();
    //     }

    //     if !app.ui_state.time_frame_menu_state.read().active {
    //         let mut time_frame_menu_state = app.ui_state.time_frame_menu_state.write();
    //         time_frame_menu_state.clear_selection()?;
    //         time_frame_menu_state.select(app.ui_state.time_frame)?;
    //     }
    // }

    cleanup_terminal();

    Ok(())
}
