//! Benchmarks for zuddy using 8-queens as an example problem
use zuddy::{SetFamily, ZddHolder};

///The position of a queen on a board.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
struct QueenPosition(u8, u8);

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

fn queens_at_row(i: u8, board_size: u8) -> impl Iterator<Item = QueenPosition> {
    (0..board_size).map(move |x| QueenPosition(i, x))
}

fn main() {
    divan::main();
}

#[divan::bench(args = [1, 4, 8, 10])]
fn n_queens(board_size: u8) -> usize {
    let mut holder = ZddHolder::<QueenPosition>::with_capacity(10000);
    let mut state = queens_at_row(0, board_size).fold(SetFamily::ZERO, |acc, x| {
        acc.union(SetFamily::singleton(x, &mut holder), &mut holder)
    });
    for i in 1..board_size {
        let mut new_state = SetFamily::ZERO;
        for queen in queens_at_row(i, board_size) {
            let mut x = state;
            for interfering_queen in queen.interferes_with_preceeding(board_size) {
                x = x.element_remainder(interfering_queen, &mut holder);
            }
            x = x.change(queen, &mut holder);
            new_state = new_state.union(x, &mut holder);
        }
        state = new_state;

        state.protect(&mut holder);
        holder.gc();
        state.unprotect(&mut holder);
    }

    state.size(&mut holder).unwrap()
}
