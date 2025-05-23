use crossterm::event::{self, Event, KeyCode};
use ratatui::Terminal;
use state::{AppState, DataLoadingState, StateChange};


mod app;
pub mod state;

pub use app::App;

pub async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> anyhow::Result<()> {
    let mut current_state: Box<dyn AppState> = Box::new(DataLoadingState::new());

    loop {
        terminal.draw(|f| current_state.ui(f, app))?;

        // Use poll with timeout to allow states to execute immediately
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match current_state.process_key(key.code, app).await {
                    StateChange::Keep => {}
                    StateChange::Change(new_state) => {
                        current_state = new_state;
                        continue; // Immediately process the new state
                    }
                    StateChange::Exit => break,
                }
            }
        } else {
            // No event, but still allow state to process (for immediate execution)
            match current_state.process_key(KeyCode::Null, app).await {
                StateChange::Keep => {}
                StateChange::Change(new_state) => {
                    current_state = new_state;
                    continue;
                }
                StateChange::Exit => break,
            }
        }
    }

    Ok(())
}
