use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiInput {
    SelectPrevious,
    SelectNext,
    Approve,
    Deny,
    Stop,
    Restart,
    Recover,
    Cancel,
    Refresh,
    Resize { columns: u16, rows: u16 },
    Exit,
    Invalid,
}

pub fn key_to_input(key: KeyEvent) -> TuiInput {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => TuiInput::Exit,
        (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => TuiInput::Exit,
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => TuiInput::SelectPrevious,
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => TuiInput::SelectNext,
        (KeyCode::Char('a'), _) => TuiInput::Approve,
        (KeyCode::Char('d'), _) => TuiInput::Deny,
        (KeyCode::Char('s'), _) => TuiInput::Stop,
        (KeyCode::Char('x'), _) => TuiInput::Cancel,
        (KeyCode::Char('e'), _) => TuiInput::Recover,
        (KeyCode::Char('R'), _) => TuiInput::Restart,
        (KeyCode::Char('r'), _) => TuiInput::Refresh,
        _ => TuiInput::Invalid,
    }
}
