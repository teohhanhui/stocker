use crate::{
    app::{App, Indicator, InputState, TimeFrame, UiState, UiTarget},
    event::{InputEvent, TextFieldEvent},
    reactive::StreamExt as ReactiveStreamExt,
    stock::Stock,
};
use argh::FromArgs;
use async_std::stream::{self, StreamExt};
use chrono::{Duration, Utc};
use crossterm::{
    cursor,
    event::{Event, EventStream, KeyCode, KeyEvent},
    execute, terminal,
};
use im::hashmap;
use reactive_rs::{Broadcast, Stream};
use std::{
    io::{self, Write},
    panic,
    sync::atomic::{self, AtomicBool},
    time,
};
use tui::{backend::CrosstermBackend, layout::Rect, Terminal};

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

    let app_start_date = Utc::now();
    let should_quit = AtomicBool::new(false);

    let ui_target_areas: Broadcast<(), (UiTarget, Option<Rect>)> = Broadcast::new();

    let active_overlay_ui_targets: Broadcast<(), Option<UiTarget>> = Broadcast::new();

    let input_events: Broadcast<(), InputEvent> = Broadcast::new();

    // input events without ticks
    let user_input_events = input_events
        .clone()
        .filter(|ev| !matches!(ev, InputEvent::Tick))
        .broadcast();

    let stock_symbol_text_field_map_mouse_funcs =
        event::ui_target_areas_to_text_field_map_mouse_funcs(
            ui_target_areas.clone(),
            hashmap! {
                Some(UiTarget::StockSymbol) => Some(TextFieldEvent::Toggle),
                Some(UiTarget::StockName) => Some(TextFieldEvent::Toggle),
                Some(UiTarget::StockSymbolInput) => None,
                None => Some(TextFieldEvent::Cancel)
            },
        )
        .broadcast();

    let stock_symbol_text_field_events = event::input_events_to_text_field_events(
        user_input_events.clone(),
        active_overlay_ui_targets.clone(),
        KeyCode::Char('s'),
        stock_symbol_text_field_map_mouse_funcs.clone(),
        |v| v.to_ascii_uppercase(),
    )
    .broadcast();

    event::connect_text_field_events_to_active_overlay_ui_targets(
        stock_symbol_text_field_events.clone(),
        active_overlay_ui_targets.clone(),
    );

    let stock_symbols = stock_symbol_text_field_events
        .clone()
        .fold(args.symbol.clone(), |acc_symbol, ev| {
            if let TextFieldEvent::Accept(symbol) = ev {
                symbol.clone()
            } else {
                acc_symbol.clone()
            }
        })
        .distinct_until_changed()
        .broadcast();

    let time_frames: Broadcast<(), TimeFrame> = Broadcast::new();

    let date_ranges = app::to_date_ranges(
        user_input_events.clone(),
        stock_symbols.clone(),
        time_frames.clone(),
        active_overlay_ui_targets.clone(),
    )
    .broadcast();

    let stock_profiles = stock::to_stock_profiles(stock_symbols.clone())
        .map(|stock_profile| Some(stock_profile.clone()))
        .broadcast();

    let stock_bar_sets = stock::to_stock_bar_sets(
        stock_symbols.clone(),
        time_frames.clone(),
        date_ranges.clone(),
    )
    .broadcast();

    let stocks = stock_symbols
        .clone()
        .combine_latest(stock_profiles.clone(), |(stock_symbol, stock_profile)| {
            (stock_symbol.clone(), stock_profile.clone())
        })
        .combine_latest(
            stock_bar_sets.clone(),
            |((stock_symbol, stock_profile), stock_bar_set)| Stock {
                bars: stock_bar_set.clone(),
                profile: stock_profile.clone(),
                symbol: stock_symbol.clone(),
                ..Stock::default()
            },
        )
        .broadcast();

    let stock_symbol_input_states =
        event::text_field_events_to_input_states(stock_symbol_text_field_events.clone())
            .broadcast();

    let ui_states = time_frames
        .clone()
        .combine_latest(date_ranges.clone(), |(time_frame, date_range)| {
            (*time_frame, date_range.clone())
        })
        .combine_latest(stock_symbol_input_states.clone(), {
            let ui_target_areas = ui_target_areas.clone();
            move |((time_frame, date_range), stock_symbol_input_state)| UiState {
                date_range: date_range.clone(),
                stock_symbol_input_state: stock_symbol_input_state.clone(),
                time_frame: *time_frame,
                ui_target_areas: ui_target_areas.clone(),
                ..UiState::default()
            }
        })
        .broadcast();

    input_events
        .clone()
        .with_latest_from(stocks.clone(), |(ev, stock)| (*ev, stock.clone()))
        .with_latest_from(ui_states.clone(), |((ev, stock), ui_state)| {
            (*ev, stock.clone(), ui_state.clone())
        })
        .subscribe(|(ev, stock, ui_state)| match ev {
            InputEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Char('q') if !ui_state.stock_symbol_input_state.active => {
                    should_quit.store(true, atomic::Ordering::Relaxed);
                }
                KeyCode::Char(_) if !ui_state.stock_symbol_input_state.active => {
                    execute!(terminal.backend_mut(), crossterm::style::Print("\x07"),).unwrap();
                }
                _ => {}
            },
            InputEvent::Tick => {
                let app = App {
                    stock: stock.clone(),
                    ui_state: ui_state.clone(),
                };
                terminal
                    .draw(|mut f| {
                        ui::draw(&mut f, &app).expect("draw failed");
                    })
                    .unwrap();
            }
            _ => {}
        });

    let input_event_stream = EventStream::new()
        .filter(|ev| match ev {
            Ok(Event::Key(_)) | Ok(Event::Mouse(_)) => true,
            _ => false,
        })
        .map(|ev| match ev {
            Ok(Event::Key(key_event)) => InputEvent::Key(key_event),
            Ok(Event::Mouse(mouse_event)) => InputEvent::Mouse(mouse_event),
            _ => unreachable!(),
        });
    let tick_stream = stream::interval(time::Duration::from_millis(TICK_RATE));
    let input_tick_stream = tick_stream.map(|()| InputEvent::Tick);
    let mut input_event_stream = input_event_stream.merge(input_tick_stream);

    // draw once before hitting the network, as it is blocking
    stocks.send(Stock {
        symbol: args.symbol.clone(),
        ..Stock::default()
    });
    ui_states.send(UiState {
        date_range: {
            args.time_frame.duration().map(|duration| {
                let end_date = app_start_date.date().and_hms(0, 0, 0) + Duration::days(1);
                (end_date - duration)..end_date
            })
        },
        time_frame: args.time_frame,
        ..UiState::default()
    });
    input_events.send(InputEvent::Tick);

    // send the initial values
    stock_symbols.send(args.symbol);
    stock_profiles.send(None);
    time_frames.send(args.time_frame);
    date_ranges.send(args.time_frame.duration().map(|duration| {
        let end_date = app_start_date.date().and_hms(0, 0, 0) + Duration::days(1);
        (end_date - duration)..end_date
    }));
    active_overlay_ui_targets.send(None);
    stock_symbol_input_states.send(InputState::default());

    while !should_quit.load(atomic::Ordering::Relaxed) {
        input_events.send(input_event_stream.next().await.unwrap());
    }

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
