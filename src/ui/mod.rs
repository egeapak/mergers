use crossterm::event::{self, Event, KeyCode};
use ratatui::Terminal;
use state::{AppState, DataLoadingState, StateChange};

/// Macro to process state changes and handle Keep/Change/Exit
macro_rules! handle_state_change {
    ($result:expr, $current_state:expr) => {
        match $result {
            StateChange::Keep => {}
            StateChange::Change(new_state) => {
                $current_state = new_state;
            }
            StateChange::Exit => break,
        }
    };
}

mod app;
#[cfg(test)]
pub mod snapshot_testing;
pub mod state;
#[cfg(test)]
pub mod testing;

pub use app::App;

pub async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> anyhow::Result<()> {
    let mut current_state: Box<dyn AppState> = app
        .initial_state
        .take()
        .unwrap_or_else(|| Box::new(DataLoadingState::new()));

    loop {
        terminal.draw(|f| current_state.ui(f, app))?;

        // Use poll with timeout to allow states to execute immediately
        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    handle_state_change!(
                        current_state.process_key(key.code, app).await,
                        current_state
                    );
                }
                Event::Mouse(mouse) => {
                    handle_state_change!(
                        current_state.process_mouse(mouse, app).await,
                        current_state
                    );
                }
                _ => {}
            }
        } else {
            // No event, but still allow state to process (for immediate execution)
            handle_state_change!(
                current_state.process_key(KeyCode::Null, app).await,
                current_state
            );
        }
    }

    Ok(())
}
