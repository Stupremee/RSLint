//! The physical parser structure.
//! This may not hold your expectations of a traditional parser,
//! the parser yields events like `Start node`, `Error`, etc.
//! These events are then applied to a `TreeSink`.

use std::cell::Cell;
use std::ops::Range;

use crate::*;

/// An extremely fast, error tolerant, completely lossless JavaScript parser
///
/// The Parser yields lower level events instead of nodes.
/// These events are then processed into a syntax tree through a [`TreeSink`] implementation.
///
/// ```
/// use rslint_parser::{
///     Parser,
///     syntax::expr,
///     tokenize,
///     TokenSource,
///     ast::GroupingExpr,
///     LosslessTreeSink,
///     SyntaxNode,
///     process,
///     AstNode
/// };
///
/// let source = "(delete b)";
///
/// // File id is used for the labels inside parser errors to report them, the file id
/// // is used to look up a file's source code and path inside of a codespan `Files` implementation.
/// let (tokens, lexer_errors) = tokenize(source, 0);
///
/// assert!(lexer_errors.is_empty());
///
/// // The parser uses a token source which manages yielding non-whitespace tokens,
/// // and giving raw tokens to allow the parser to turn completed markers into syntax nodes
/// let token_source = TokenSource::new(source, &tokens);
///
/// let mut parser = Parser::new(token_source, 0);
///
/// // Use one of the syntax parsing functions to parse an expression.
/// // This adds node and token events to the parser which are then used to make a node.
/// // A completed marker marks the start and end indices in the events vec which signify
/// // the Start event, and the Finish event.
/// // Completed markers can be turned into an ast node with parse_marker on the parser
/// let completed_marker = expr::expr(&mut parser).unwrap();
///
/// parser.parse_marker::<GroupingExpr>(&completed_marker);
///
/// // Make a new text tree sink, its job is assembling events into a rowan GreenNode.
/// // At each point (Start, Token, Finish, Error) it also consumes whitespace.
/// // Other abstractions can also yield lossy syntax nodes if whitespace is not wanted.
/// // Swap this for a LossyTreeSink for a lossy AST result.
/// let mut sink = LosslessTreeSink::new(source, &tokens);
///
/// // Consume the parser and get its events, then apply them to a tree sink with `process`.
/// process(&mut sink, parser.finish());
///
/// let (green, errors) = sink.finish();
///
/// assert!(errors.is_empty());
///
/// // Make a new SyntaxNode from the green node
/// let untyped_node = SyntaxNode::new_root(green);
///
/// assert!(GroupingExpr::can_cast(untyped_node.kind()));
///
/// // Convert the untyped SyntaxNode into a typed AST node
/// let typed_expr = GroupingExpr::cast(untyped_node).unwrap();
///
/// assert_eq!(typed_expr.inner().unwrap().syntax().text(), "delete b");
/// ```
pub struct Parser<'t> {
    pub file_id: usize,
    tokens: TokenSource<'t>,
    pub(crate) events: Vec<Event>,
    // This is for tracking if the parser is infinitely recursing.
    // We use a cell so we dont need &mut self on `nth()`
    steps: Cell<u32>,
    pub state: ParserState,
}

impl<'t> Parser<'t> {
    /// Make a new parser configured to parse a script
    pub fn new(tokens: TokenSource<'t>, file_id: usize) -> Parser<'t> {
        Parser {
            file_id,
            tokens,
            events: vec![],
            steps: Cell::new(0),
            state: ParserState::default(),
        }
    }

    /// Make a new parser configured to parse a module
    pub fn new_module(tokens: TokenSource<'t>, file_id: usize) -> Parser<'t> {
        Parser {
            file_id,
            tokens,
            events: vec![],
            steps: Cell::new(0),
            state: ParserState::module(),
        }
    }

    /// Get the source code of a token
    pub fn token_src(&self, token: &Token) -> &str {
        self.tokens
            .source()
            .get(token.range.to_owned())
            .expect("Token range and src mismatch")
    }

    /// Consume the parser and return the list of events it produced
    pub fn finish(self) -> Vec<Event> {
        self.events
    }

    /// Get the current token kind of the parser
    pub fn cur(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// Get the current token of the parser
    pub fn cur_tok(&self) -> Token {
        self.nth_tok(0)
    }

    /// Look ahead at a token and get its kind, **The max lookahead is 4**.  
    ///
    /// # Panics
    /// This method panics if the lookahead is higher than `4`,
    /// or if the parser has run this method more than 10m times, as it is a sign of infinite recursion
    pub fn nth(&self, n: usize) -> SyntaxKind {
        assert!(n <= 4);

        let steps = self.steps.get();
        assert!(
            steps <= 10_000_000,
            "The parser seems to be recursing forever"
        );
        self.steps.set(steps + 1);

        self.tokens.lookahead_nth(n).kind
    }

    /// Look ahead at a token, **The max lookahead is 4**.  
    ///
    /// # Panics
    /// This method panics if the lookahead is higher than `4`,
    /// or if the parser has run this method more than 10m times, as it is a sign of infinite recursion
    pub fn nth_tok(&self, n: usize) -> Token {
        assert!(n <= 4);

        let steps = self.steps.get();
        assert!(
            steps <= 10_000_000,
            "The parser seems to be recursing forever"
        );
        self.steps.set(steps + 1);

        self.tokens.lookahead_nth(n)
    }

    /// Check if the parser is currently at a specific token
    pub fn at(&self, kind: SyntaxKind) -> bool {
        self.nth_at(0, kind)
    }

    /// Check if a token lookahead is something, `n` must be smaller or equal to `4`
    pub fn nth_at(&self, n: usize, kind: SyntaxKind) -> bool {
        self.tokens.lookahead_nth(n).kind == kind
    }

    /// Consume the next token if `kind` matches.
    pub fn eat(&mut self, kind: SyntaxKind) -> bool {
        if !self.at(kind) {
            return false;
        }
        self.do_bump(kind);
        true
    }

    /// Recover from an error with a recovery set or by using a `{` or `}`.
    pub fn err_recover(
        &mut self,
        error: impl Into<ParserError>,
        recovery: TokenSet,
        include_braces: bool,
    ) {
        match self.cur() {
            T!['{'] | T!['}'] if include_braces => {
                self.error(error);
                return;
            }
            _ => (),
        }

        if self.at_ts(recovery) {
            self.error(error);
            return;
        }

        let m = self.start();
        self.error(error);
        self.bump_any();
        m.complete(self, SyntaxKind::ERROR);
    }

    /// Recover from an error but don't add an error to the events
    pub fn err_recover_no_err(&mut self, recovery: TokenSet, include_braces: bool) {
        match self.cur() {
            T!['{'] | T!['}'] if include_braces => {
                return;
            }
            _ => (),
        }

        if self.at_ts(recovery) {
            return;
        }

        let m = self.start();
        self.bump_any();
        m.complete(self, SyntaxKind::ERROR);
    }

    /// Starts a new node in the syntax tree. All nodes and tokens
    /// consumed between the `start` and the corresponding `Marker::complete`
    /// belong to the same node.
    pub fn start(&mut self) -> Marker {
        let pos = self.events.len() as u32;
        self.push_event(Event::tombstone(self.tokens.cur_pos()));
        Marker::new(pos, self.tokens.cur_pos())
    }

    /// Check if there was a linebreak before a lookahead
    pub fn has_linebreak_before_n(&self, n: usize) -> bool {
        self.tokens.had_linebreak_before_nth(n)
    }

    /// Consume the next token if `kind` matches.
    pub fn bump(&mut self, kind: SyntaxKind) {
        assert!(self.eat(kind));
    }

    /// Consume any token but cast it as a different kind
    pub fn bump_remap(&mut self, kind: SyntaxKind) {
        self.do_bump(kind)
    }

    /// Advances the parser by one token
    pub fn bump_any(&mut self) {
        let kind = self.nth(0);
        if kind == SyntaxKind::EOF {
            return;
        }
        self.do_bump(kind)
    }

    /// Make a new error builder with `error` severity
    pub fn err_builder(&self, message: &str) -> ErrorBuilder {
        ErrorBuilder::error(self.file_id, message)
    }

    /// Add an error event
    pub fn error(&mut self, err: impl Into<ParserError>) {
        self.push_event(Event::Error { err: err.into() });
    }

    /// Check if the parser's current token is contained in a token set
    pub fn at_ts(&self, kinds: TokenSet) -> bool {
        kinds.contains(self.cur())
    }

    fn do_bump(&mut self, kind: SyntaxKind) {
        self.tokens.bump();

        self.push_event(Event::Token { kind });
    }

    fn push_event(&mut self, event: Event) {
        self.events.push(event)
    }

    /// Get the source code of the parser's current token.
    ///
    /// # Panics
    /// This method panics if the token range and source code range mismatch
    pub fn cur_src(&self) -> &str {
        self.tokens
            .source()
            .get(self.nth_tok(0).range)
            .expect("Parser source and tokens mismatch")
    }

    /// Try to eat a specific token kind, if the kind is not there then add an error to the events stack.
    pub fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            true
        } else {
            let err = if self.cur() == SyntaxKind::EOF {
                self.err_builder(&format!(
                    "expected `{}` but instead the file ends",
                    kind.to_string()
                        .map(|x| x.to_string())
                        .unwrap_or_else(|| format!("{:?}", kind))
                ))
                .primary(self.cur_tok().range, "the file ends here")
            } else {
                self.err_builder(&format!(
                    "expected `{}` but instead found `{}`",
                    kind.to_string()
                        .map(|x| x.to_string())
                        .unwrap_or_else(|| format!("{:?}", kind)),
                    self.cur_src()
                ))
                .primary(self.cur_tok().range, "unexpected")
            };

            self.error(err);
            false
        }
    }

    /// Get the byte index range of a completed marker for error reporting.
    pub fn marker_range(&self, marker: &CompletedMarker) -> Range<usize> {
        match self.events[marker.start_pos as usize] {
            Event::Start { start, .. } => match self.events[marker.finish_pos as usize] {
                Event::Finish { end } => start..end,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    /// Parse a completed marker into an ast node.
    ///
    /// # Panics
    /// Panics if the AST node represented by the marker does not match the generic
    pub fn parse_marker<T: AstNode>(&self, marker: &CompletedMarker) -> T {
        let events = self
            .events
            .get(marker.old_start as usize..(marker.finish_pos as usize + 1))
            .expect("Marker out of bounds")
            .to_vec();

        let start = match self.events[marker.old_start as usize] {
            Event::Start { start, .. } => start,
            _ => unreachable!(),
        };

        let mut sink =
            LosslessTreeSink::with_offset(self.tokens.source(), &self.tokens.raw_tokens, start);
        process(&mut sink, events);
        T::cast(SyntaxNode::new_root(sink.finish().0))
            .expect("Marker was parsed to the wrong ast node")
    }

    /// Get the source code of a range
    pub fn source(&self, range: TextRange) -> &str {
        &self.tokens.source()[range]
    }

    /// Rewind the token position back to a former position
    pub fn rewind(&mut self, pos: usize) {
        self.tokens.rewind(pos)
    }

    /// Get the current index of the last event
    pub fn cur_event_pos(&self) -> usize {
        self.events.len().saturating_sub(1)
    }

    /// Remove `amount` events from the parser
    pub fn drain_events(&mut self, amount: usize) {
        self.events.truncate(self.events.len() - amount);
    }

    /// Get the current token position
    pub fn token_pos(&self) -> usize {
        self.tokens.cur_token_idx()
    }

    /// Make a new error builder with warning severity
    pub fn warning_builder(&self, message: &str) -> ErrorBuilder {
        ErrorBuilder::warning(self.file_id, message)
    }

    /// Bump and add an error event
    pub fn err_and_bump(&mut self, err: impl Into<ParserError>) {
        let m = self.start();
        self.bump_any();
        m.complete(self, SyntaxKind::ERROR);
        self.error(err);
    }
}

/// A structure signifying the start of parsing of a syntax tree node
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Marker {
    /// The index in the events list
    pub pos: u32,
    /// The byte index where the node starts
    pub start: usize,
    pub old_start: u32,
}

impl Marker {
    pub fn new(pos: u32, start: usize) -> Marker {
        Marker {
            pos,
            start,
            old_start: pos,
        }
    }

    pub(crate) fn old_start(mut self, old: u32) -> Self {
        if self.old_start >= old {
            self.old_start = old;
        };
        self
    }

    /// Finishes the syntax tree node and assigns `kind` to it,
    /// and mark the create a `CompletedMarker` for possible future
    /// operation like `.precede()` to deal with forward_parent.
    pub fn complete(self, p: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        let idx = self.pos as usize;
        match p.events[idx] {
            Event::Start {
                kind: ref mut slot, ..
            } => {
                *slot = kind;
            }
            _ => unreachable!(),
        }
        let finish_pos = p.events.len() as u32;
        p.push_event(Event::Finish {
            end: p.tokens.last().map(|t| t.range.end).unwrap_or(0),
        });
        CompletedMarker::new(self.pos, finish_pos, kind).old_start(self.old_start)
    }

    /// Abandons the syntax tree node. All its children
    /// are attached to its parent instead.
    pub fn abandon(self, p: &mut Parser) {
        let idx = self.pos as usize;
        if idx == p.events.len() - 1 {
            match p.events.pop() {
                Some(Event::Start {
                    kind: SyntaxKind::TOMBSTONE,
                    forward_parent: None,
                    ..
                }) => (),
                _ => unreachable!(),
            }
        }
    }
}

/// A structure signifying a completed node
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct CompletedMarker {
    start_pos: u32,
    // Hack for parsing completed markers which have been preceded
    // This should be redone completely in the future
    pub(crate) old_start: u32,
    finish_pos: u32,
    kind: SyntaxKind,
}

impl CompletedMarker {
    pub fn new(start_pos: u32, finish_pos: u32, kind: SyntaxKind) -> Self {
        CompletedMarker {
            start_pos,
            old_start: start_pos,
            finish_pos,
            kind,
        }
    }

    pub(crate) fn old_start(mut self, old: u32) -> Self {
        // For multiple precedes we should not update the old start
        if self.old_start >= old {
            self.old_start = old;
        };
        self
    }

    /// Change the kind of node this marker represents
    pub fn change_kind(&mut self, p: &mut Parser, new_kind: SyntaxKind) {
        self.kind = new_kind;
        match p
            .events
            .get_mut(self.start_pos as usize)
            .expect("Finish position of marker is OOB")
        {
            Event::Start { kind, .. } => {
                *kind = new_kind;
            }
            _ => unreachable!(),
        }
    }

    // Get the correct offset range in source code of an item inside of a parsed marker
    pub fn offset_range(&self, p: &Parser, range: TextRange) -> TextRange {
        let offset = self.range(p).start();
        TextRange::new(range.start() + offset, range.end() + offset)
    }

    /// Get the range of the marker
    pub fn range(&self, p: &Parser) -> TextRange {
        let start = match p.events[self.old_start as usize] {
            Event::Start { start, .. } => start as u32,
            _ => unreachable!(),
        };
        let end = match p.events[self.finish_pos as usize] {
            Event::Finish { end } => end as u32,
            _ => unreachable!(),
        };
        TextRange::new(start.into(), end.into())
    }

    /// Get the underlying text of a marker
    pub fn text<'a>(&self, p: &'a Parser) -> &'a str {
        &p.tokens.source()[self.range(p)]
    }

    /// This method allows to create a new node which starts
    /// *before* the current one. That is, parser could start
    /// node `A`, then complete it, and then after parsing the
    /// whole `A`, decide that it should have started some node
    /// `B` before starting `A`. `precede` allows to do exactly
    /// that. See also docs about `forward_parent` in `Event::Start`.
    ///
    /// Given completed events `[START, FINISH]` and its corresponding
    /// `CompletedMarker(pos: 0, _)`.
    /// Append a new `START` events as `[START, FINISH, NEWSTART]`,
    /// then mark `NEWSTART` as `START`'s parent with saving its relative
    /// distance to `NEWSTART` into forward_parent(=2 in this case);
    pub fn precede(self, p: &mut Parser) -> Marker {
        let new_pos = p.start();
        let idx = self.start_pos as usize;
        match p.events[idx] {
            Event::Start {
                ref mut forward_parent,
                ..
            } => {
                *forward_parent = Some(new_pos.pos - self.start_pos);
            }
            _ => unreachable!(),
        }
        new_pos.old_start(self.old_start as u32)
    }

    /// Undo this completion and turns into a `Marker`
    pub fn undo_completion(self, p: &mut Parser) -> Marker {
        let start_idx = self.start_pos as usize;
        let finish_idx = self.finish_pos as usize;
        let start_pos;

        match p.events[start_idx] {
            Event::Start {
                ref mut kind,
                forward_parent: None,
                start,
            } => {
                start_pos = start;
                *kind = SyntaxKind::TOMBSTONE
            }
            _ => unreachable!(),
        }
        match p.events[finish_idx] {
            ref mut slot @ Event::Finish { .. } => *slot = Event::tombstone(start_pos),
            _ => unreachable!(),
        }
        Marker::new(self.start_pos, start_pos)
    }

    pub fn kind(&self) -> SyntaxKind {
        self.kind
    }
}
