//! The mobile core in a phone-sized desktop window: the mouse is one finger. Swipe across the rope to catch it,
//! pull, let go.

fn main() {
    if let Err(e) = sim_mobile::preview() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}
