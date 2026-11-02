#!/usr/bin/env python3
"""Start positions for datagen games of attacks on the king: kings castled on opposite
wings, both queens on, by move 25, close to balanced. One per run of such positions in
the data (about one a game), plus those of the popularpos book and the ends of the lines
of books/aggressive.txt. Needs python-chess.

  scripts/attack-starts.py <out.epd> <popularpos.epd> <aggressive.txt> <datagen dirs>...

For 1.5.0, from the generations of 0.10.0 and 1.1.1:

  scripts/attack-starts.py data/attack-starts.epd engines/books/popularpos_lichess_v3.epd \
      books/aggressive.txt data/0.10.0-random data/0.10.0-book data/1.1.1-random data/1.1.1-book
"""
import sys
from pathlib import Path

import chess

MAX_MOVE = 25
MAX_SCORE = 150


def wing(board_field, king, back_rank):
    f = 0
    for c in board_field.split("/")[back_rank]:
        if c.isdigit():
            f += int(c)
        elif c == king:
            return "short" if f >= 6 else "long" if f <= 2 else None
        else:
            f += 1
    return None


def opposite_castling(board_field):
    white, black = wing(board_field, "K", 7), wing(board_field, "k", 0)
    return (white is not None and black is not None and white != black
            and "Q" in board_field and "q" in board_field)


def main(out, popularpos, aggressive, dirs):
    starts = {}
    for path in sorted(p for d in dirs for p in Path(d).glob("*.txt")):
        before = len(starts)
        previous = False
        for line in open(path):
            fen, score, _ = line.split(" | ")
            fields = fen.split(" ")
            current = (opposite_castling(fields[0]) and int(fields[5]) <= MAX_MOVE
                       and abs(int(score)) <= MAX_SCORE)
            if current and not previous:
                starts.setdefault(" ".join(fields[:4]), None)
            previous = current
        print(f"{path}: {len(starts) - before}")

    book = [line.split()[:4] for line in open(popularpos) if line.strip()]
    kept = [" ".join(f) for f in book if opposite_castling(f[0])]
    starts.update(dict.fromkeys(kept))
    print(f"popularpos: {len(kept)}")

    ends = 0
    for line in open(aggressive):
        line = line.split("#")[0]
        if not line.strip():
            continue
        board = chess.Board()
        for mv in line.split()[2:]:
            board.push_uci(mv)
        starts.setdefault(" ".join(board.fen().split()[:4]), None)
        ends += 1
    print(f"aggressive book: {ends} line ends")

    Path(out).write_text("".join(f"{s}\n" for s in starts))
    print(f"{len(starts)} positions in {out}")


if __name__ == "__main__":
    if len(sys.argv) < 5:
        print(__doc__)
        sys.exit(1)
    main(sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:])
