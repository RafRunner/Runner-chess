use crate::{
    attacks::{KING_ATTACKS, KNIGHT_ATTACKS, PAWN_ATTACKS, bishop_attacks, rook_attacks},
    bitboard::BitBoard,
    board::Board,
    chess_move::{Move, MoveKind},
    piece::{Color, Piece, PieceKind},
    square::Square,
};

/// What each piece is worth to the static exchange evaluation. The king can't be
/// taken: it only ever recaptures last
const VALUES: [i32; 6] = [100, 300, 300, 500, 900, 0];

impl Board {
    /// Static exchange evaluation: whether `mv` wins at least `threshold` of material
    /// once both sides took back on its square, each with its cheapest piece first,
    /// and either free to stop when going on would lose. The pieces behind the ones
    /// that take, on the same line, join in. Pins and checks are ignored, and so is
    /// promoting along the way: a promotion is always good enough, castling wins
    /// nothing.
    pub fn see(&self, mv: Move, threshold: i32) -> bool {
        let (from, to) = (mv.from(), mv.to());
        let mut occupied = self.all_pieces_bb() ^ from.bb();

        let victim = match mv.kind() {
            MoveKind::Promotion(_) | MoveKind::PromotionCapture(_) => return true,
            MoveKind::Castle => return threshold <= 0,
            MoveKind::EnPassant => {
                occupied ^= mv.get_ep_square(self.side_to_move()).bb();
                VALUES[PieceKind::Pawn]
            }
            _ => self.piece_at(to).map_or(0, |piece| VALUES[piece.kind()]),
        };

        // what the side to move is still short of, if the opponent takes back now
        let mut swap = victim - threshold;
        if swap < 0 {
            return false;
        }
        let moving = self.piece_at(from).map_or(0, |piece| VALUES[piece.kind()]);
        swap = moving - swap;
        if swap <= 0 {
            return true;
        }

        let diagonal = self.both(PieceKind::Bishop) | self.both(PieceKind::Queen);
        let straight = self.both(PieceKind::Rook) | self.both(PieceKind::Queen);
        let mut attackers = self.all_attackers(to, occupied);
        let mut side = self.side_to_move();
        // whether the side that moved comes out ahead, as the exchange stands
        let mut wins = true;

        loop {
            side = side.oposite();
            attackers &= occupied;
            let ours = attackers & self.by_color(side);
            if ours.is_empty() {
                break;
            }
            wins = !wins;

            let (kind, mut bb) = PieceKind::ALL
                .into_iter()
                .map(|kind| (kind, ours & self.pieces(Piece::new(side, kind))))
                .find(|(_, bb)| !bb.is_empty())
                .expect("one of the pieces attacks");

            if kind == PieceKind::King {
                // it can only take if nothing takes it back
                if !(attackers & !self.by_color(side)).is_empty() {
                    wins = !wins;
                }
                break;
            }

            // the exchange stops where taking on can't come out ahead anymore
            swap = VALUES[kind] - swap;
            if swap < i32::from(wins) {
                break;
            }

            occupied ^= bb.next().expect("not empty").bb();
            if matches!(kind, PieceKind::Pawn | PieceKind::Bishop | PieceKind::Queen) {
                attackers |= bishop_attacks(to, occupied) & diagonal;
            }
            if matches!(kind, PieceKind::Rook | PieceKind::Queen) {
                attackers |= rook_attacks(to, occupied) & straight;
            }
        }

        wins
    }

    /// The pieces of both colors that attack `sq`, as if only `occupied` had pieces
    fn all_attackers(&self, sq: Square, occupied: BitBoard) -> BitBoard {
        let pawns = |color: Color| {
            PAWN_ATTACKS[color.oposite()][sq] & self.pieces(Piece::new(color, PieceKind::Pawn))
        };

        pawns(Color::White)
            | pawns(Color::Black)
            | (KNIGHT_ATTACKS[sq] & self.both(PieceKind::Knight))
            | (KING_ATTACKS[sq] & self.both(PieceKind::King))
            | (bishop_attacks(sq, occupied)
                & (self.both(PieceKind::Bishop) | self.both(PieceKind::Queen)))
            | (rook_attacks(sq, occupied)
                & (self.both(PieceKind::Rook) | self.both(PieceKind::Queen)))
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{board, find_move, mirror_fen, mirror_uci};

    /// The material `uci` wins from `fen` by exchange, found as the highest threshold
    /// that `see` still accepts, the same for the mirrored position
    fn see_value(fen: &str, uci: &str) -> i32 {
        let value = |fen: &str, uci: &str| {
            let board = board(fen);
            let mv = find_move(&board, uci);
            (-1000..=1000)
                .rev()
                .find(|&threshold| board.see(mv, threshold))
                .unwrap()
        };
        let result = value(fen, uci);
        assert_eq!(result, value(&mirror_fen(fen), &mirror_uci(uci)), "{fen}");
        result
    }

    #[test]
    fn an_undefended_piece_is_won_whole() {
        assert_eq!(
            see_value("1k1r4/1pp4p/p7/4p3/8/P5P1/1PP4P/2K1R3 w - - 0 1", "e1e5"),
            100
        );
    }

    #[test]
    fn a_defended_pawn_costs_the_knight() {
        // Nxe5 Nxe5 Rxe5 Bxe5: the knight for a pawn, and stopping after Nxe5 is best
        let fen = "1k1r3q/1ppn3p/p4b2/4p3/8/P2N2P1/1PP1R1BP/2K1Q3 w - - 0 1";
        assert_eq!(see_value(fen, "d3e5"), -200);
    }

    #[test]
    fn an_even_trade_is_zero() {
        // Nxd5 Nxd5: a knight each
        assert_eq!(see_value("4k3/8/5n2/3n4/8/4N3/8/4K3 w - - 0 1", "e3d5"), 0);
    }

    #[test]
    fn the_pieces_behind_join_in() {
        // Rxd5 exd5 is answered by the rook behind: only a pawn for the rook still,
        // so the queen behind doesn't help White
        let fen = "3rk3/8/4p3/3p4/8/8/3R4/3QK3 w - - 0 1";
        assert_eq!(see_value(fen, "d2d5"), -400);
        // with no pawn defending, the rooks trade and White keeps the pawn
        let fen = "3rk3/8/8/3p4/8/8/3R4/3QK3 w - - 0 1";
        assert_eq!(see_value(fen, "d2d5"), 100);
    }

    #[test]
    fn the_king_only_takes_what_is_not_defended() {
        // Qxe2 Kxe2: the queen for a pawn
        assert_eq!(see_value("4k3/8/8/8/8/8/4P2q/4K3 b - - 0 1", "h2e2"), -800);
        // Rxe7+ Kxe7, unless the other rook defends it
        assert_eq!(see_value("4k3/4p3/8/8/8/8/8/4R1K1 w - - 0 1", "e1e7"), -400);
        assert_eq!(
            see_value("4k3/4p3/8/8/8/8/4R3/4R1K1 w - - 0 1", "e2e7"),
            100
        );
    }

    #[test]
    fn quiet_moves_to_attacked_squares_lose_the_piece() {
        // Nf3 walks into exf3
        assert_eq!(see_value("4k3/8/8/8/4p3/8/8/4K1N1 w - - 0 1", "g1f3"), -300);
        assert_eq!(see_value("4k3/8/8/8/4p3/8/8/4K1N1 w - - 0 1", "g1h3"), 0);
    }

    #[test]
    fn en_passant_takes_the_pawn_beside() {
        assert_eq!(see_value("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1", "e5d6"), 100);
    }
}
