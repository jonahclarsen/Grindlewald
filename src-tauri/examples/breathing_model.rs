use grindlewald_lib::breathing::{PerceptualCycle, color_at_position, minimum_cycle_seconds};

fn main() {
    let seconds = minimum_cycle_seconds(1);
    for base in [0.1, 0.4, 1.0] {
        let plan = PerceptualCycle::new(0, 1, seconds, base).unwrap();
        let (index, fastest) = plan
            .frames
            .iter()
            .enumerate()
            .min_by_key(|(_, frame)| frame.interval)
            .unwrap();
        let next = &plan.frames[(index + 1) % plan.frames.len()];
        let slowest = plan
            .frames
            .iter()
            .map(|frame| frame.interval)
            .max()
            .unwrap();
        let extra = plan
            .frames
            .iter()
            .enumerate()
            .filter(|(index, frame)| {
                frame.brightness != plan.frames[(index + 1) % plan.frames.len()].brightness
            })
            .count();
        println!(
            "base={:.0}% cycle={}s fastest={} -> {} brightness={}/255 -> {}/255 interval={:.6}ms rate={:.3}Hz slowest={:.6}ms brightness_changes={}",
            base * 100.0,
            seconds,
            color_at_position(fastest.position),
            color_at_position(next.position),
            fastest.brightness,
            next.brightness,
            fastest.interval.as_secs_f64() * 1000.0,
            1.0 / fastest.interval.as_secs_f64(),
            slowest.as_secs_f64() * 1000.0,
            extra
        );
    }
}
