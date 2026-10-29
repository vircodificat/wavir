/// Mutable simulation state, kept separate from the static [`Style`].
#[derive(Debug, Clone)]
pub struct State {
    /// Current simulation time; drives the "now" cursor.
    pub now_ps: u64,
    pub viewport_start_ps: u64,
    pub viewport_end_ps: u64,
    /// End of the simulation, hardcoded to the natural next timestep for the
    /// clock after its last event.
    pub end_ps: u64,
    /// Interval (in picoseconds) by which the viewport jumps.
    pub jump_interval_ps: u64,

    pub signals: Vec<Signal>,

    /// Index of the currently selected signal, if any. `None` means there
    /// are no signals or the selection is deselected.
    pub selected_signal: Option<usize>,
}

/// A signal value: raw 64-bit payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Value(pub u64);

/// The kind of signal being displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    /// Two levels: low (0) and high (non-zero).
    Bit,
    /// Multi-bit value.
    Value,
}

/// A change of a signal's value at a point in time.
#[derive(Debug, Clone)]
pub struct SignalEvent {
    pub time_ps: u64,
    pub new_value: Value,
}

/// One named signal with its event history.
#[derive(Debug, Clone)]
pub struct Signal {
    pub name: String,
    pub signal_type: SignalType,
    pub events: Vec<SignalEvent>,
}

impl State {
    /// Time of the last event of the given signal strictly before
    /// `now_ps`, or `None` if there is none.
    pub fn prev_event_ps(&self, signal_idx: usize, now_ps: u64) -> Option<u64> {
        let events = &self.signals.get(signal_idx)?.events;
        events.iter().rev().map(|e| e.time_ps).find(|t| *t < now_ps)
    }

    /// Time of the first event of the given signal strictly after
    /// `now_ps`, or `None` if there is none.
    pub fn next_event_ps(&self, signal_idx: usize, now_ps: u64) -> Option<u64> {
        let events = &self.signals.get(signal_idx)?.events;
        events.iter().map(|e| e.time_ps).find(|t| *t > now_ps)
    }

    /// Hardcoded sample state.
    pub fn sample() -> Self {
        let ev = |time_ps: u64, v: u64| SignalEvent {
            time_ps,
            new_value: Value(v),
        };

        // A perfectly regular clock: starts low at t=0 and toggles every
        // half period across the whole viewport.
        let half_period = 50;
        let clk_events: Vec<SignalEvent> = (0..)
            .map_while(|k| {
                let t = k * half_period;
                (t < 1000).then(|| ev(t, u64::from(k % 2 == 1)))
            })
            .collect();

        Self {
            now_ps: 500,
            viewport_start_ps: 0,
            viewport_end_ps: 1000,
            // The clock's last event is at t=950; the next natural timestep
            // is one more half-period later.
            end_ps: 1000,
            // The clock's period.
            jump_interval_ps: 2 * half_period,
            selected_signal: None,
            signals: vec![
                Signal {
                    name: "clock".into(),
                    signal_type: SignalType::Bit,
                    events: clk_events,
                },
                Signal {
                    name: "req".into(),
                    signal_type: SignalType::Bit,
                    events: vec![ev(0, 0), ev(100, 1), ev(600, 0)],
                },
                Signal {
                    name: "ack".into(),
                    signal_type: SignalType::Bit,
                    events: vec![ev(0, 0), ev(250, 1), ev(750, 0)],
                },
                Signal {
                    name: "data".into(),
                    signal_type: SignalType::Value,
                    events: vec![ev(0, 0x42), ev(300, 0x2a), ev(700, 0x7)],
                },
            ],
        }
    }
}
