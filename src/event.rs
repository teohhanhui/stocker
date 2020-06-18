use crate::app::InputState;
use crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use reactive_rs::Stream;

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

pub fn create_text_field_event_stream<'a, S, F>(
    event_stream: S,
    activation_key_code: KeyCode,
    mut map_value_func: F,
) -> impl Stream<'a, Item = (InputState, Option<TextFieldEvent>)>
where
    S: Stream<'a, Item = InputEvent>,
    F: 'a + FnMut(String) -> String,
{
    event_stream.fold(
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
                KeyCode::Enter if acc_input_state.active && !acc_input_state.value.is_empty() => (
                    InputState::default(),
                    Some(TextFieldEvent::Accept(
                        acc_input_state.value.trim().to_owned(),
                    )),
                ),
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
}
