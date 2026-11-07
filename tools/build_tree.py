#!/usr/bin/env python3
"""Grow an opening tree with Stockfish and write it as an annotated PGN.

The seed PGN contains the lines you care about (main line + variations).
Every move gets the engine evaluation of the resulting position as a comment
(White's point of view, e.g. { +0.35 }). From every leaf of the seed tree the
script then expands the tree with the engine's candidate moves:

* For the side that branches (--branch): the best move plus every further
  MultiPV move until the first one that is more than --worse-cp centipawns
  worse than the best.
* For the other side: only the best reply.
* A branch stops when |eval| > --cutoff-cp, when the position is game over,
  or after the given --last-move (full move number).

Engine results are cached by FEN (--cache), so an interrupted run continues
where it stopped. The output PGN is rewritten atomically after every new move,
so it can be opened in a viewer while the analysis is still running.

Requirements: python-chess (pip install chess) and a UCI engine (Stockfish).
"""
import argparse
import json
import os
import time

import chess
import chess.engine
import chess.pgn

MATE = 100000


class TreeBuilder:
    def __init__(self, args):
        self.a = args
        self.cache = {}
        if os.path.exists(args.cache):
            with open(args.cache) as f:
                self.cache = json.load(f)
        self.engine = None
        self.game = None
        self.analysed = 0

    # ---------- output ----------
    def log(self, msg):
        print(time.strftime("%H:%M:%S"), msg, flush=True)

    def _atomic_write(self, path, text):
        tmp = path + ".tmp"
        with open(tmp, "w", encoding="utf-8") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)

    def save_pgn(self):
        self._atomic_write(self.a.output, str(self.game) + "\n")

    def save_cache(self):
        self._atomic_write(self.a.cache, json.dumps(self.cache))

    # ---------- engine ----------
    def analyse(self, board):
        """List of (uci, centipawns from White's view), best first."""
        key = board.fen()
        if key in self.cache:
            return self.cache[key]
        if self.engine is None:
            self.engine = chess.engine.SimpleEngine.popen_uci(self.a.engine)
            self.engine.configure({"Threads": self.a.threads, "Hash": self.a.hash})
        self.analysed += 1
        self.log(f"#{self.analysed} analysing {key}")
        infos = self.engine.analyse(board, chess.engine.Limit(time=self.a.time), multipv=self.a.multipv)
        res = []
        for info in infos:
            if info.get("pv"):
                cp = info["score"].white().score(mate_score=MATE)
                res.append((info["pv"][0].uci(), cp))
        self.cache[key] = res
        self.save_cache()
        return res

    # ---------- tree ----------
    @staticmethod
    def fmt(cp):
        if abs(cp) >= MATE - 1000:
            n = MATE - abs(cp)
            return f"#{n}" if cp > 0 else f"#-{n}"
        return f"{cp / 100:+.2f}"

    def set_eval(self, node, cp):
        node.comment = self.fmt(cp)
        self.save_pgn()

    def branches(self, color):
        return self.a.branch == "both" or (self.a.branch == "white") == (color == chess.WHITE)

    def expand(self, node, cp):
        board = node.board()
        if abs(cp) > self.a.cutoff_cp or board.is_game_over():
            return
        if board.fullmove_number > self.a.last_move:
            return
        lines = self.analyse(board)
        if not lines:
            return
        if self.branches(board.turn):
            # Scores are from White's view: Black's best is the lowest.
            sign = 1 if board.turn == chess.WHITE else -1
            best = sign * lines[0][1]
            chosen = []
            for uci, mcp in lines:
                if best - sign * mcp > self.a.worse_cp:
                    break
                chosen.append((uci, mcp))
        else:
            chosen = lines[:1]
        children = []
        for uci, mcp in chosen:
            move = chess.Move.from_uci(uci)
            child = node.variation(move) if node.has_variation(move) else node.add_variation(move)
            self.set_eval(child, mcp)
            children.append((child, mcp))
        for child, mcp in children:
            self.expand(child, mcp)

    def walk_seed(self, node):
        """Annotate the seed tree and expand its leaves."""
        for child in list(node.variations):
            board = child.board()
            if board.is_game_over():
                continue
            cp = self.analyse(board)[0][1]
            self.set_eval(child, cp)
            if child.variations:
                self.walk_seed(child)
            else:
                self.expand(child, cp)

    def run(self):
        with open(self.a.input, encoding="utf-8") as f:
            self.game = chess.pgn.read_game(f)
        if self.game is None:
            raise SystemExit(f"no game found in {self.a.input}")
        try:
            self.walk_seed(self.game)
            self.save_pgn()
            self.log(f"done, {self.analysed} new positions analysed")
        finally:
            if self.engine:
                self.engine.quit()


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("input", help="seed PGN with the lines to start from (never modified)")
    p.add_argument("output", help="annotated PGN to write (must differ from input)")
    p.add_argument("--engine", default="stockfish", help="path to the UCI engine (default: stockfish on PATH)")
    p.add_argument("--time", type=float, default=30.0, help="seconds per position (default 30)")
    p.add_argument("--multipv", type=int, default=5, help="candidate moves per position (default 5)")
    p.add_argument("--threads", type=int, default=max(1, (os.cpu_count() or 2) - 2))
    p.add_argument("--hash", type=int, default=1024, help="hash size in MB (default 1024)")
    p.add_argument("--branch", choices=["white", "black", "both"], default="white",
                   help="side whose good alternatives are all kept (default white)")
    p.add_argument("--worse-cp", type=int, default=50,
                   help="drop candidates more than this many centipawns worse than the best (default 50)")
    p.add_argument("--cutoff-cp", type=int, default=100,
                   help="stop a branch when |eval| exceeds this (default 100)")
    p.add_argument("--last-move", type=int, default=12, help="last full move number to add (default 12)")
    p.add_argument("--cache", default=None, help="engine cache file (default: <output>.cache.json)")
    a = p.parse_args()
    if os.path.abspath(a.input) == os.path.abspath(a.output):
        raise SystemExit("input and output must be different files")
    a.cache = a.cache or a.output + ".cache.json"
    TreeBuilder(a).run()


if __name__ == "__main__":
    main()
