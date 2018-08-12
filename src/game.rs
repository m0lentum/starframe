use std::time;

pub trait Game {
    fn continuous_update(&self, elapsed: time::Duration);
    fn fixed_update(&self);
    fn draw(&self);

    fn run(&self, frame_duration: time::Duration) {
        let mut last_updated = time::Instant::now();
        let mut since_fixed_update = time::Duration::new(0, 0);

        loop {
            let elapsed = last_updated.elapsed();
            last_updated = time::Instant::now();

            since_fixed_update += elapsed;
            if since_fixed_update >= frame_duration {
                // TODO: if we can't run the game fast enough to meet the desired framerate,
                // the framerate will fluctuate randomly. This should be controlled somehow
                since_fixed_update -= frame_duration;

                self.fixed_update();
            }

            self.continuous_update(elapsed);

            self.draw();
        }
    }
}
