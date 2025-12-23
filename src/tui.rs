use crate::system_state::SystemState;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    symbols,
    widgets::{Axis, Block, Borders, Chart, Dataset},
    DefaultTerminal, Frame,
};
use std::time::{Duration, Instant};

const MAX_SAMPLES: usize = 300;

struct CpuLoadGraph {
    data: Vec<f64>,
    counter: u32,
    last_tick: Instant,
}

impl CpuLoadGraph {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            counter: 0,
            last_tick: Instant::now(),
        }
    }

    fn add_sample(&mut self, value: f64) {
        self.data.push(value);
        if self.data.len() > MAX_SAMPLES {
            self.data.remove(0);
        }
    }

    fn get_chart_data(&self) -> Vec<(f64, f64)> {
        let mut points = Vec::new();

        for (i, &load) in self.data.iter().enumerate() {
            let x = i as f64;
            let actual_load = if load < 0.5 { 0.1 } else { load };

            let steps = ((actual_load / 2.0).max(10.0) as usize).min(50);
            for step in 0..=steps {
                let y = (step as f64 / steps as f64) * actual_load;
                points.push((x, y));
            }
        }

        points
    }
}

pub fn run_tui(mut terminal: DefaultTerminal, system_state: &SystemState) -> Result<()> {
    terminal.clear()?;
    let mut app = CpuLoadGraph::new();
    let tick_rate = Duration::from_millis(300);

    loop {
        terminal.draw(|f| render(f, &app))?;

        let timeout = tick_rate
            .checked_sub(app.last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break Ok(());
                }
            }
        }

        if app.last_tick.elapsed() >= tick_rate {
            let load = system_state.cpu_states.read_cpu_load().unwrap_or(0.0);
            app.add_sample(load);
            app.counter += 1;
            app.last_tick = Instant::now();
        }
    }
}

fn render(frame: &mut Frame, app: &CpuLoadGraph) {
    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .margin(1)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(frame.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50)])
        .split(horizontal_chunks[1]);

    let current_load = app.data.last().copied().unwrap_or(0.0);
    let chart_data = app.get_chart_data();

    let dataset = Dataset::default()
        .name(format!("CPU: {:.1}%", current_load))
        .marker(symbols::Marker::Block)
        .style(ratatui::style::Color::Cyan)
        .data(&chart_data);

    let chart = Chart::new(vec![dataset])
        .block(Block::default().borders(Borders::ALL))
        .x_axis(Axis::default().bounds([0.0, MAX_SAMPLES as f64]))
        .y_axis(Axis::default().bounds([0.0, 100.0]));

    frame.render_widget(chart, chunks[0]);
}
