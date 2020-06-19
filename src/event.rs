use crate::{app::InputState, reactive::StreamExt};
use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use futures::executor;
use reactive_rs::Stream;
use yahoo_finance::Profile;

#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Tick,
}

#[derive(Clone, Debug)]
pub enum TextFieldEvent {
    Accept(String),
    Activate,
    Cancel,
    Modify(String),
}

pub fn input_events_to_text_field_events<'a, S, F, C>(
    input_events: S,
    activation_key_code: KeyCode,
    mut map_value_func: F,
) -> impl Stream<'a, Item = TextFieldEvent, Context = C>
where
    S: Stream<'a, Item = InputEvent, Context = C>,
    F: 'a + FnMut(String) -> String,
{
    input_events
        .fold(
            (InputState::default(), None),
            move |(acc_input_state, _), ev| match ev {
                InputEvent::Key(KeyEvent { code, .. }) => match code {
                    KeyCode::Backspace if acc_input_state.active => {
                        let mut value = acc_input_state.value.clone();
                        value.pop();
                        let value = map_value_func(value);
                        (
                            InputState {
                                value: value.clone(),
                                ..*acc_input_state
                            },
                            Some(TextFieldEvent::Modify(value)),
                        )
                    }
                    KeyCode::Enter
                        if acc_input_state.active && !acc_input_state.value.is_empty() =>
                    {
                        (
                            InputState::default(),
                            Some(TextFieldEvent::Accept(
                                acc_input_state.value.trim().to_owned(),
                            )),
                        )
                    }
                    KeyCode::Esc if acc_input_state.active => {
                        (InputState::default(), Some(TextFieldEvent::Cancel))
                    }
                    &key_code if key_code == activation_key_code && !acc_input_state.active => (
                        InputState {
                            active: true,
                            value: acc_input_state.value.clone(),
                        },
                        Some(TextFieldEvent::Activate),
                    ),
                    KeyCode::Char(c) if acc_input_state.active => {
                        let mut value = acc_input_state.value.clone();
                        value.push(*c);
                        let value = map_value_func(value);
                        (
                            InputState {
                                value: value.clone(),
                                ..*acc_input_state
                            },
                            Some(TextFieldEvent::Modify(value)),
                        )
                    }
                    _ => (acc_input_state.clone(), None),
                },
                _ => (acc_input_state.clone(), None),
            },
        )
        .filter_map(|(_, ev)| ev.clone())
}

pub fn text_field_events_to_input_states<'a, S, C>(
    text_field_events: S,
) -> impl Stream<'a, Item = InputState, Context = C>
where
    S: Stream<'a, Item = TextFieldEvent, Context = C>,
    C: Default,
{
    text_field_events
        .fold(InputState::default(), |_, ev| match ev {
            TextFieldEvent::Activate => InputState {
                active: true,
                value: "".to_owned(),
            },
            TextFieldEvent::Modify(value) => InputState {
                active: true,
                value: value.clone(),
            },
            TextFieldEvent::Accept(_) | TextFieldEvent::Cancel => InputState {
                active: false,
                value: "".to_owned(),
            },
        })
        .start_with(InputState::default())
}

pub fn stock_symbols_to_stock_profiles<'a, S>(stock_symbols: S) -> StockSymbolToStockProfile<S>
where
    S: Stream<'a, Item = String>,
{
    StockSymbolToStockProfile { stock_symbols }
}

pub struct StockSymbolToStockProfile<S> {
    stock_symbols: S,
}

impl<'a, S> Stream<'a> for StockSymbolToStockProfile<S>
where
    S: Stream<'a, Item = String>,
{
    type Context = S::Context;
    type Item = Profile;

    fn subscribe_ctx<O>(self, mut observer: O)
    where
        O: 'a + FnMut(&Self::Context, &Self::Item),
    {
        self.stock_symbols.subscribe_ctx(move |ctx, symbol| {
            let profile =
                executor::block_on(Profile::load(symbol.as_str())).expect("Profile load failed");

            observer(ctx, &profile);
        });
    }
}
