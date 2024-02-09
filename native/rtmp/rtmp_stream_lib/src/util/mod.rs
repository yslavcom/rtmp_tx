pub mod util;

macro_rules! trace {
    ($($args: expr),*) => {
        print!("TRACE: file: {}, line: {}", file!(), line!());
        $(
            print!(", {}: {}", stringify!($args), $args);
        )*
        println!(""); // to get a new line at the end
    }
}

pub fn ttt(){}