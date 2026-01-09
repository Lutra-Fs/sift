//! Sift GUI - Graphical User Interface
//!
//! Iced-based native GUI for managing MCP servers and skills.

use iced::widget::{column, text, text_input};
use iced::{Element, Task};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> iced::Result {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sift_gui=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    iced::run(
        "Sift - MCP & Skills Manager",
        SiftGui::update,
        SiftGui::view,
    )
}

#[derive(Debug, Clone)]
enum Message {
    InputChanged(String),
}

#[derive(Default)]
struct SiftGui {
    input_value: String,
}

impl SiftGui {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::InputChanged(value) => {
                self.input_value = value;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            text("Sift - MCP & Skills Manager").size(24),
            text("GUI interface coming soon").size(14),
            text_input("Type something...", &self.input_value).on_input(Message::InputChanged),
        ]
        .padding(20)
        .spacing(10)
        .into()
    }
}
