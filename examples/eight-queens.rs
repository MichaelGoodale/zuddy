//! An example of the usage of ZDDs using the 8 queens problem.

use rand::prelude::*;
use zuddy::{SetFamily, ZddHolder};

///The position of a queen on a board.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
struct QueenPosition(u8, u8);

impl std::fmt::Display for QueenPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}_{}", self.0, self.1)
    }
}

impl QueenPosition {
    ///Get the the queens on earlier rows that would be affected by this queen.
    fn interferes_with_preceeding(self, board_size: u8) -> Vec<QueenPosition> {
        let mut queens = Vec::new();

        //We only need backwards directions since we are going row by row.
        //This also means we can ignore horizontal movement.
        let dirs = [(-1, 0), (-1, 1), (-1, -1)];
        let board_size = i16::from(board_size);

        for (dx, dy) in dirs {
            let mut x = i16::from(self.0) + dx;
            let mut y = i16::from(self.1) + dy;

            while x >= 0 && x < i16::from(self.0) && y >= 0 && y < board_size {
                queens.push(QueenPosition(
                    u8::try_from(x).unwrap(),
                    u8::try_from(y).unwrap(),
                ));
                x += dx;
                y += dy;
            }
        }
        queens
    }
}

fn print_horizontal_line(board_size: u8) {
    for _ in 0..board_size {
        print!("+---");
    }
    println!("+");
}

fn print_solution(queens: &[QueenPosition], board_size: u8) {
    for y in 0..board_size {
        print_horizontal_line(board_size);
        for x in 0..board_size {
            let has_queen = queens
                .iter()
                .any(|&QueenPosition(qx, qy)| qx == x && qy == y);

            if has_queen {
                print!("| ♛ ");
            } else {
                print!("|   ");
            }
        }
        println!("|");
    }
    print_horizontal_line(board_size);
}

fn queens_at_row(i: u8, board_size: u8) -> impl Iterator<Item = QueenPosition> {
    (0..board_size).map(move |x| QueenPosition(i, x))
}

fn n_queens(board_size: u8, rng: &mut impl Rng) -> usize {
    let holder = ZddHolder::<QueenPosition>::with_capacity(10000);
    let mut state = queens_at_row(0, board_size).fold(holder.zero(), |acc, x| {
        acc.union(SetFamily::singleton(x, &holder))
    });
    for i in 1..board_size {
        let mut new_state = holder.zero();
        for queen in queens_at_row(i, board_size) {
            let mut x = state.clone();
            for interfering_queen in queen.interferes_with_preceeding(board_size) {
                x = x.element_remainder(interfering_queen);
            }
            x = x.change(queen);
            new_state = new_state.union(x);
        }
        state = new_state;
    }

    let n_sol = state.size().unwrap();

    println!(
        "{board_size}-Queens has {n_sol} solutions! (ZDD size: {}, holder size: {})\nHere's a random one for you:",
        state.n_nodes(),
        holder.n_nodes()
    );
    let sampled_queens = state.sample(rng);
    print_solution(&sampled_queens, board_size);

    n_sol
}

fn main() {
    let mut rng = ThreadRng::default();
    //no solution for n=2,3
    for (n, n_sol) in [1, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        .into_iter()
        .zip([1, 2, 10, 4, 40, 92, 352, 724, 2680, 14200])
    {
        let n_sol_calc = n_queens(n, &mut rng);
        assert_eq!(n_sol_calc, n_sol);
    }
}
