//! Minimal PGN parser that turns the *first* game of a file into a move tree
//! (main line + all nested variations), keeping each move's comment and the
//! evaluation extracted from it.

use shakmaty::fen::Fen;
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, Color, Position, Square};

pub struct Node {
    /// SAN of the move leading to this node (empty for the root).
    pub san: String,
    /// From/to squares of that move (king's real destination for castling).
    pub from_to: Option<(Square, Square)>,
    /// Position after the move.
    pub pos: Chess,
    pub comment: String,
    /// Evaluation parsed from the comment, from White's point of view.
    pub eval: Option<Eval>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Eval {
    Cp(f32),
    Mate(i32),
}

impl Eval {
    pub fn text(self) -> String {
        match self {
            Eval::Cp(v) => {
                let v = if v.abs() < 0.005 { 0.0 } else { v };
                format!("{v:+.2}")
            }
            Eval::Mate(n) => format!("#{n}"),
        }
    }
}

pub struct Tree {
    pub headers: Vec<(String, String)>,
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    /// SAN path from the root to `node`.
    pub fn path_of(&self, mut node: usize) -> Vec<String> {
        let mut path = Vec::new();
        while let Some(p) = self.nodes[node].parent {
            path.push(self.nodes[node].san.clone());
            node = p;
        }
        path.reverse();
        path
    }

    /// Follow a SAN path as far as it exists in this tree.
    pub fn follow(&self, path: &[String]) -> usize {
        let mut cur = 0;
        for san in path {
            match self.nodes[cur].children.iter().find(|&&c| &self.nodes[c].san == san) {
                Some(&c) => cur = c,
                None => break,
            }
        }
        cur
    }

    /// Move-number prefix for the move leading to `node`, e.g. "4." or "4...".
    pub fn move_prefix(&self, node: usize) -> String {
        let parent = self.nodes[node].parent.unwrap_or(0);
        let pos = &self.nodes[parent].pos;
        let n = pos.fullmoves().get();
        if pos.turn() == Color::White { format!("{n}.") } else { format!("{n}...") }
    }
}

pub fn parse(text: &str) -> Result<Tree, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut headers = Vec::new();
    let mut rest = text;
    // Header section: lines starting with '['.
    loop {
        let trimmed = rest.trim_start();
        if !trimmed.starts_with('[') {
            rest = trimmed;
            break;
        }
        let end = trimmed.find(']').ok_or("unterminated header")?;
        let inner = &trimmed[1..end];
        if let Some(sp) = inner.find(char::is_whitespace) {
            let key = inner[..sp].to_string();
            let val = inner[sp..].trim().trim_matches('"').to_string();
            headers.push((key, val));
        }
        rest = &trimmed[end + 1..];
    }

    let start: Chess = match headers.iter().find(|(k, _)| k == "FEN") {
        Some((_, fen)) => Fen::from_ascii(fen.as_bytes())
            .map_err(|e| format!("bad FEN header: {e}"))?
            .into_position(CastlingMode::Standard)
            .map_err(|e| format!("illegal FEN header: {e}"))?,
        None => Chess::default(),
    };

    let mut nodes = vec![Node {
        san: String::new(),
        from_to: None,
        pos: start,
        comment: String::new(),
        eval: None,
        parent: None,
        children: Vec::new(),
    }];

    let mut cur = 0usize;
    let mut stack: Vec<usize> = Vec::new();
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '{' => {
                let start = i + 1;
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                let comment: String = chars[start..i.min(chars.len())].iter().collect();
                add_comment(&mut nodes[cur], comment.trim());
                i += 1;
            }
            ';' => {
                let start = i + 1;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                let comment: String = chars[start..i].iter().collect();
                add_comment(&mut nodes[cur], comment.trim());
            }
            '(' => {
                stack.push(cur);
                cur = nodes[cur].parent.ok_or("variation without a preceding move")?;
                i += 1;
            }
            ')' => {
                cur = stack.pop().ok_or("unbalanced ')'")?;
                i += 1;
            }
            '[' => break, // next game starts: only the first game is used
            _ => {
                let start = i;
                while i < chars.len() && !chars[i].is_whitespace() && !"{}();".contains(chars[i]) {
                    i += 1;
                }
                let tok: String = chars[start..i].iter().collect();
                if tok.starts_with('$') || matches!(tok.as_str(), "1-0" | "0-1" | "1/2-1/2" | "*") {
                    continue;
                }
                // Strip move numbers like "12." / "12..." (possibly glued to the move).
                let san = tok.trim_start_matches(|ch: char| ch.is_ascii_digit());
                let san = if san.len() < tok.len() { san.trim_start_matches('.') } else { &tok };
                let san = san.trim_end_matches(['!', '?']);
                if san.is_empty() {
                    continue;
                }
                cur = add_move(&mut nodes, cur, san)?;
            }
        }
    }

    Ok(Tree { headers, nodes })
}

fn add_comment(node: &mut Node, comment: &str) {
    if comment.is_empty() {
        return;
    }
    if !node.comment.is_empty() {
        node.comment.push(' ');
    }
    node.comment.push_str(comment);
    if node.eval.is_none() {
        node.eval = parse_eval(comment);
    }
}

fn add_move(nodes: &mut Vec<Node>, cur: usize, san: &str) -> Result<usize, String> {
    let pos = nodes[cur].pos.clone();
    let parsed = SanPlus::from_ascii(san.as_bytes()).map_err(|e| format!("bad move '{san}': {e}"))?;
    let m = parsed
        .san
        .to_move(&pos)
        .map_err(|e| format!("illegal move '{san}' in {}: {e}", Fen::from_position(&pos, shakmaty::EnPassantMode::Legal)))?;
    let san_norm = SanPlus::from_move(pos.clone(), m).to_string();
    if let Some(&c) = nodes[cur].children.iter().find(|&&c| nodes[c].san == san_norm) {
        return Ok(c);
    }
    let from_to = match m.to_uci(CastlingMode::Standard) {
        UciMove::Normal { from, to, .. } => Some((from, to)),
        _ => None,
    };
    let mut after = pos;
    after.play_unchecked(m);
    let idx = nodes.len();
    nodes.push(Node {
        san: san_norm,
        from_to,
        pos: after,
        comment: String::new(),
        eval: None,
        parent: Some(cur),
        children: Vec::new(),
    });
    nodes[cur].children.push(idx);
    Ok(idx)
}

/// Extract an evaluation from a comment: `[%eval 0.35]`, `[%eval #-3]`,
/// or a plain leading token like `+0.35`, `-1.20`, `#3`.
pub fn parse_eval(comment: &str) -> Option<Eval> {
    if let Some(p) = comment.find("[%eval") {
        let rest = &comment[p + 6..];
        let val = rest.trim_start().split([']', ',', ' ']).next().unwrap_or("");
        return parse_eval_token(val);
    }
    comment.split_whitespace().find_map(parse_eval_token)
}

fn parse_eval_token(tok: &str) -> Option<Eval> {
    if let Some(m) = tok.strip_prefix('#') {
        return m.trim_start_matches('+').parse::<i32>().ok().map(Eval::Mate);
    }
    if !tok.contains('.') {
        return None;
    }
    let first = tok.chars().next()?;
    if !(first == '+' || first == '-' || first.is_ascii_digit()) {
        return None;
    }
    tok.trim_start_matches('+').parse::<f32>().ok().map(Eval::Cp)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Event \"x\"]\n[Result \"*\"]\n\n1. d4 { +0.15 } 1... d5 { +0.22 } 2. e4 { -0.67 } \
        2... dxe4 3. Nc3 e5 4. Qh5 { -0.81 } ( 4. Nxe4 { -0.20 } 4... Qxd4 5. Bd3 ( 5. Qxd4 { -0.51 } ) ) \
        ( 4. dxe5 ) 4... exd4 { #-3 } *\n";

    #[test]
    fn parses_tree_and_evals() {
        let t = parse(SAMPLE).unwrap();
        assert_eq!(t.header("Event"), Some("x"));
        let d4 = t.nodes[0].children[0];
        assert_eq!(t.nodes[d4].san, "d4");
        assert_eq!(t.nodes[d4].eval, Some(Eval::Cp(0.15)));
        let after_e5 = t.follow(&["d4", "d5", "e4", "dxe4", "Nc3", "e5"].map(String::from));
        let kids: Vec<&str> = t.nodes[after_e5].children.iter().map(|&c| t.nodes[c].san.as_str()).collect();
        assert_eq!(kids, ["Qh5", "Nxe4", "dxe5"]);
        let nxe4 = t.nodes[after_e5].children[1];
        let qxd4 = t.nodes[nxe4].children[0];
        assert_eq!(t.nodes[qxd4].children.len(), 2); // 5.Bd3 and the alternative 5.Qxd4
        assert_eq!(t.move_prefix(nxe4), "4.");
        let qh5 = t.nodes[after_e5].children[0];
        let exd4 = t.nodes[qh5].children[0];
        assert_eq!(t.nodes[exd4].eval, Some(Eval::Mate(-3)));
        assert_eq!(t.move_prefix(exd4), "4...");
    }

    #[test]
    fn path_round_trip() {
        let t = parse(SAMPLE).unwrap();
        let n = t.follow(&["d4", "d5", "e4", "dxe4", "Nc3", "e5", "Nxe4", "Qxd4", "Qxd4"].map(String::from));
        assert_eq!(t.path_of(n).last().unwrap(), "Qxd4");
        assert_eq!(t.follow(&t.path_of(n)), n);
    }

    #[test]
    fn eval_formats() {
        assert_eq!(parse_eval("+0.35 (d34) [%eval 0.35]"), Some(Eval::Cp(0.35)));
        assert_eq!(parse_eval("[%eval -1.2,30]"), Some(Eval::Cp(-1.2)));
        assert_eq!(parse_eval("nice move"), None);
        assert_eq!(Eval::Cp(0.35).text(), "+0.35");
        assert_eq!(Eval::Mate(-2).text(), "#-2");
    }

    #[test]
    fn real_file_parses_if_present() {
        if let Ok(s) = std::fs::read_to_string("../lemberger/variante.pgn") {
            let t = parse(&s).unwrap();
            assert!(t.nodes.len() > 10);
        }
    }
}
