use argh::FromArgs;
use chrono::{Duration, TimeZone, Utc};
use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyEvent},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use math::round;
use std::cmp::Ordering;
use std::error::Error;
use std::io::{self, Write};
use std::time::{Duration as StdDuration, Instant};
use tui::{
    backend::{self, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Text},
    Frame, Terminal,
};
use yahoo_finance::{history, Bar, Interval, Profile, Quote, Timestamped};

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
struct UiState {
    last_tick: Instant,
    time_frame: Interval,
}

#[derive(Debug)]
enum InputEvent {
    Input(char),
    Tick,
}

#[derive(Debug)]
struct Stock {
    bars: Vec<Bar>,
    profile: Option<Profile>,
    quote: Option<Quote>,
    symbol: String,
}

fn read_input(ui_state: &mut UiState) -> Result<InputEvent, Box<dyn Error>> {
    let tick_rate = StdDuration::from_millis(TICK_RATE);

    loop {
        if event::poll(tick_rate)? {
            if let Ok(CrosstermEvent::Key(KeyEvent {
                code: KeyCode::Char(c),
                ..
            })) = event::read()
            {
                return Ok(InputEvent::Input(c));
            }
        }

        if ui_state.last_tick.elapsed() >= tick_rate {
            ui_state.last_tick = Instant::now();

            return Ok(InputEvent::Tick);
        }
    }
}

fn draw<B: backend::Backend>(
    terminal: &mut Terminal<B>,
    ui_state: &UiState,
    stock: &Stock,
) -> io::Result<()> {
    terminal.draw(|mut f| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![
                Constraint::Length(2),
                Constraint::Min(5),
                Constraint::Length(2),
            ])
            .split(f.size());

        draw_header_block(&mut f, ui_state, stock, chunks[0]);
        draw_body_block(&mut f, ui_state, stock, chunks[1]);
        draw_footer_block(&mut f, ui_state, stock, chunks[2]);
    })
}

fn draw_header_block<B: backend::Backend>(
    f: &mut Frame<B>,
    ui_state: &UiState,
    stock: &Stock,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(1)
        .constraints(vec![Constraint::Length(10), Constraint::Length(20)])
        .split(area);

    let header_base_style = Style::default().fg(Color::White).bg(Color::DarkGray);

    let header_block = Block::default().style(header_base_style);
    f.render_widget(header_block, area);

    let symbol_texts = vec![Text::raw(stock.symbol.clone())];
    let symbol_paragraph = Paragraph::new(symbol_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style.clone().modifier(Modifier::BOLD));
    f.render_widget(symbol_paragraph, chunks[0]);

    let name_texts = vec![Text::raw(match &stock.profile {
        Some(Profile::Company(company)) => company.name.clone(),
        Some(Profile::Fund(fund)) => fund.name.clone(),
        None => "".to_owned(),
    })];
    let name_paragraph = Paragraph::new(name_texts.iter())
        .block(Block::default().style(header_base_style))
        .style(header_base_style);
    f.render_widget(name_paragraph, chunks[1]);
}

fn draw_body_block<B: backend::Backend>(
    f: &mut Frame<B>,
    ui_state: &UiState,
    stock: &Stock,
    area: Rect,
) {
    let historical_prices_data = stock
        .bars
        .iter()
        .map(|bar| {
            (
                bar.timestamp_seconds() as f64,
                round::half_to_even(bar.close, 2),
            )
        })
        .collect::<Vec<_>>();
    let historical_prices_datasets = [Dataset::default()
        .marker(Marker::Braille)
        .style(Style::default().fg(Color::Cyan))
        .graph_type(GraphType::Line)
        .data(&historical_prices_data)];
    let min_timestamp = historical_prices_data
        .clone()
        .into_iter()
        .map(|(date, _)| date)
        .min_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal))
        .unwrap_or((Utc::now() - Duration::days(7)).timestamp() as f64);
    let price_limit_low = historical_prices_data
        .clone()
        .into_iter()
        .map(|(_, price)| price)
        .min_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal))
        .unwrap_or(0_f64);
    let price_limit_low = round::floor(price_limit_low, 0);
    let price_limit_high = historical_prices_data
        .clone()
        .into_iter()
        .map(|(_, price)| price)
        .max_by(|x, y| x.partial_cmp(y).unwrap_or(Ordering::Equal))
        .unwrap_or(1_000_000_f64);
    let price_limit_high = round::ceil(price_limit_high, 0);

    let x_axis_bounds = [min_timestamp, Utc::now().timestamp() as f64];
    let x_axis_labels = [
        Utc.timestamp(min_timestamp as i64, 0)
            .format("%Y-%m-%d")
            .to_string(),
        Utc::now().format("%Y-%m-%d").to_string(),
    ];
    let y_axis_bounds = [price_limit_low, price_limit_high];
    let y_axis_labels = [price_limit_low.to_string(), price_limit_high.to_string()];

    let historical_prices_chart = Chart::default()
        .block(
            Block::default()
                .title("Historical Prices")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Gray)),
        )
        .x_axis(Axis::default().bounds(x_axis_bounds).labels(&x_axis_labels))
        .y_axis(Axis::default().bounds(y_axis_bounds).labels(&y_axis_labels))
        .datasets(&historical_prices_datasets);
    f.render_widget(historical_prices_chart, area);
}

fn draw_footer_block<B: backend::Backend>(
    f: &mut Frame<B>,
    ui_state: &UiState,
    stock: &Stock,
    area: Rect,
) {
    let time_frame_texts = vec![
        Text::styled("Time frame: ", Style::default().fg(Color::Gray)),
        Text::raw(ui_state.time_frame.to_string()),
    ];
    let time_frame_paragraph = Paragraph::new(time_frame_texts.iter())
        .block(Block::default())
        .alignment(Alignment::Right);
    f.render_widget(time_frame_paragraph, area);
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

    terminal.clear()?;

    let mut ui_state = UiState {
        last_tick: Instant::now(),
        time_frame: args.time_frame,
    };

    let mut stock = Stock {
        bars: vec![],
        profile: None,
        quote: None,
        symbol: args.symbol,
    };

    stock.profile = Profile::load(stock.symbol.as_str())
        .await
        .map_or(None, Some);

    stock.bars = history::retrieve_interval(
        stock.symbol.as_str(),
        time_frame_as_interval(ui_state.time_frame.to_string().as_str())?, // hacky hack
    )
    .await?;

    loop {
        match read_input(&mut ui_state)? {
            InputEvent::Input('q') => {
                break;
            }
            InputEvent::Input(_) => {}
            InputEvent::Tick => {}
        };

        draw(&mut terminal, &ui_state, &stock)?;
    }

    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    terminal::disable_raw_mode()?;

    Ok(())
}
