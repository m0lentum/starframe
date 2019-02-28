mod fluidbox;
mod gameloop;

fn main() {
    let res = gameloop::init();
    gameloop::begin(res);
}
