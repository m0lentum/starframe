#[macro_use]
extern crate microprofile;

mod controls;
mod gameloop;

fn main() {
    microprofile::init!();
    microprofile::set_enable_all_groups!(true);

    let res = gameloop::init();
    gameloop::begin(res);

    //microprofile::dump_file_immediately!("profile.html", "");
    microprofile::shutdown!();
}
