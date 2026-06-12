import { TT, Token, Expr, Stmt, Param, Program, SwitchCase } from './types';
import { isTypeKeyword } from './lexer';

export class ParseError extends Error {
  constructor(msg: string, public token: Token) {
    super(`[${token.line}:${token.col}] ${msg}`);
  }
}

export function parse(tokens: Token[]): Program {
  let pos = 0;

  function peek(): Token { return tokens[pos] ?? { type: TT.Eof, value: '', line: 0, col: 0 }; }
  function advance(): Token { return tokens[pos++]; }
  function expect(tt: TT, msg?: string): Token {
    const t = peek();
    if (t.type !== tt) throw new ParseError(msg ?? `Expected ${TT[tt]}, got '${t.value}'`, t);
    return advance();
  }
  function match(tt: TT): boolean {
    if (peek().type === tt) { advance(); return true; }
    return false;
  }
  function at(tt: TT): boolean { return peek().type === tt; }

  function isTypeStart(): boolean {
    return isTypeKeyword(peek().type) || (at(TT.Ident) && tokens[pos + 1]?.type === TT.Ident);
  }

  function parseType(): string {
    const t = advance();
    let name = t.value;
    // struct Name or just a type keyword
    if (t.type === TT.KwStruct && at(TT.Ident)) {
      name = 'struct ' + advance().value;
    }
    return name;
  }

  function parseProgram(): Program {
    const stmts: Stmt[] = [];
    while (!at(TT.Eof)) {
      try {
        stmts.push(parseTopLevel());
      } catch (e) {
        // Skip to next semicolon or brace on error
        while (!at(TT.Eof) && !at(TT.Semi) && !at(TT.RBrace)) advance();
        if (at(TT.Semi)) advance();
        if (e instanceof ParseError) {
          console.warn('Parse warning:', e.message);
        } else {
          throw e;
        }
      }
    }
    return { stmts };
  }

  function parseTopLevel(): Stmt {
    // #include
    if (at(TT.KwInclude)) {
      const t = advance();
      return { kind: 'include', path: t.value };
    }

    // struct declaration
    if (at(TT.KwStruct) && tokens[pos + 1]?.type === TT.Ident && tokens[pos + 2]?.type === TT.LBrace) {
      return parseStructDecl();
    }

    // const declaration
    if (at(TT.KwConst)) {
      return parseConstDecl();
    }

    // Function or variable declaration (type name ...)
    if (isTypeStart()) {
      return parseDeclOrFunc();
    }

    // Fallback: expression statement
    return parseStmt();
  }

  function parseStructDecl(): Stmt {
    expect(TT.KwStruct);
    const name = expect(TT.Ident).value;
    expect(TT.LBrace);
    const fields: Param[] = [];
    while (!at(TT.RBrace) && !at(TT.Eof)) {
      const type = parseType();
      const fieldName = expect(TT.Ident).value;
      expect(TT.Semi);
      fields.push({ type, name: fieldName });
    }
    expect(TT.RBrace);
    match(TT.Semi);
    return { kind: 'struct_decl', name, fields };
  }

  function parseConstDecl(): Stmt {
    expect(TT.KwConst);
    const type = parseType();
    const name = expect(TT.Ident).value;
    expect(TT.Eq);
    const init = parseExpr();
    expect(TT.Semi);
    return { kind: 'const_decl', type, name, init };
  }

  function parseDeclOrFunc(): Stmt {
    const type = parseType();
    const name = expect(TT.Ident).value;

    // Function declaration
    if (at(TT.LParen)) {
      return parseFuncDecl(type, name);
    }

    // Variable declaration
    let init: Expr | null = null;
    if (match(TT.Eq)) {
      init = parseExpr();
    }
    expect(TT.Semi);
    return { kind: 'var_decl', type, name, init };
  }

  function parseFuncDecl(retType: string, name: string): Stmt {
    expect(TT.LParen);
    const params = parseParams();
    expect(TT.RParen);

    // Forward declaration (just a prototype)
    if (match(TT.Semi)) {
      return { kind: 'func_decl', retType, name, params, body: [] };
    }

    const body = parseBlock();
    return { kind: 'func_decl', retType, name, params, body };
  }

  function parseParams(): Param[] {
    const params: Param[] = [];
    if (at(TT.RParen)) return params;
    do {
      const type = parseType();
      const name = expect(TT.Ident).value;
      let default_: Expr | undefined;
      if (match(TT.Eq)) {
        default_ = parseExpr();
      }
      params.push({ type, name, default_ });
    } while (match(TT.Comma));
    return params;
  }

  function parseBlock(): Stmt[] {
    expect(TT.LBrace);
    const stmts: Stmt[] = [];
    while (!at(TT.RBrace) && !at(TT.Eof)) {
      stmts.push(parseStmt());
    }
    expect(TT.RBrace);
    return stmts;
  }

  function parseStmt(): Stmt {
    // Block
    if (at(TT.LBrace)) {
      return { kind: 'block', stmts: parseBlock() };
    }

    // If
    if (match(TT.KwIf)) return parseIf();

    // For
    if (match(TT.KwFor)) return parseFor();

    // While
    if (match(TT.KwWhile)) return parseWhile();

    // Do-while
    if (match(TT.KwDo)) return parseDoWhile();

    // Switch
    if (match(TT.KwSwitch)) return parseSwitch();

    // Return
    if (match(TT.KwReturn)) {
      const value = at(TT.Semi) ? null : parseExpr();
      expect(TT.Semi);
      return { kind: 'return', value };
    }

    // Break
    if (match(TT.KwBreak)) { expect(TT.Semi); return { kind: 'break' }; }

    // Continue
    if (match(TT.KwContinue)) { expect(TT.Semi); return { kind: 'continue' }; }

    // Const
    if (at(TT.KwConst)) return parseConstDecl();

    // Variable declaration (type name = ...)
    if (isTypeStart()) {
      const type = parseType();
      const name = expect(TT.Ident).value;

      // Could be a function decl inside a block (NWScript allows this for prototypes)
      if (at(TT.LParen) && peek().type === TT.LParen) {
        // Skip function prototype inside blocks
        while (!at(TT.Semi) && !at(TT.Eof)) advance();
        match(TT.Semi);
        return { kind: 'expr_stmt', expr: { kind: 'int_lit', value: 0 } };
      }

      let init: Expr | null = null;
      if (match(TT.Eq)) {
        init = parseExpr();
      }
      expect(TT.Semi);
      return { kind: 'var_decl', type, name, init };
    }

    // Expression statement
    const expr = parseExpr();
    expect(TT.Semi);
    return { kind: 'expr_stmt', expr };
  }

  function parseIf(): Stmt {
    expect(TT.LParen);
    const cond = parseExpr();
    expect(TT.RParen);
    const then = at(TT.LBrace) ? parseBlock() : [parseStmt()];
    let else_: Stmt[] | null = null;
    if (match(TT.KwElse)) {
      if (at(TT.KwIf)) {
        // else if — wrap in array
        else_ = [{ kind: 'if', cond: { kind: 'bool_lit', value: true }, then: [], else_: null, ...parseIf() as any }];
        // Actually, let's just recurse properly
        const elifStmt = parseIf();
        else_ = [elifStmt];
      } else {
        else_ = at(TT.LBrace) ? parseBlock() : [parseStmt()];
      }
    }
    return { kind: 'if', cond, then, else_ };
  }

  function parseFor(): Stmt {
    expect(TT.LParen);
    let init: Stmt | null = null;
    if (!at(TT.Semi)) {
      if (isTypeStart()) {
        const type = parseType();
        const name = expect(TT.Ident).value;
        let initExpr: Expr | null = null;
        if (match(TT.Eq)) initExpr = parseExpr();
        init = { kind: 'var_decl', type, name, init: initExpr };
      } else {
        init = { kind: 'expr_stmt', expr: parseExpr() };
      }
    }
    expect(TT.Semi);
    const cond = at(TT.Semi) ? null : parseExpr();
    expect(TT.Semi);
    const incr = at(TT.RParen) ? null : parseExpr();
    expect(TT.RParen);
    const body = at(TT.LBrace) ? parseBlock() : [parseStmt()];
    return { kind: 'for', init, cond, incr, body };
  }

  function parseWhile(): Stmt {
    expect(TT.LParen);
    const cond = parseExpr();
    expect(TT.RParen);
    const body = at(TT.LBrace) ? parseBlock() : [parseStmt()];
    return { kind: 'while', cond, body };
  }

  function parseDoWhile(): Stmt {
    const body = parseBlock();
    expect(TT.KwWhile);
    expect(TT.LParen);
    const cond = parseExpr();
    expect(TT.RParen);
    expect(TT.Semi);
    return { kind: 'do_while', cond, body };
  }

  function parseSwitch(): Stmt {
    expect(TT.LParen);
    const expr = parseExpr();
    expect(TT.RParen);
    expect(TT.LBrace);
    const cases: SwitchCase[] = [];
    while (!at(TT.RBrace) && !at(TT.Eof)) {
      if (match(TT.KwCase)) {
        const value = parseExpr();
        expect(TT.Colon);
        const body: Stmt[] = [];
        while (!at(TT.KwCase) && !at(TT.KwDefault) && !at(TT.RBrace) && !at(TT.Eof)) {
          body.push(parseStmt());
        }
        cases.push({ value, body });
      } else if (match(TT.KwDefault)) {
        expect(TT.Colon);
        const body: Stmt[] = [];
        while (!at(TT.KwCase) && !at(TT.KwDefault) && !at(TT.RBrace) && !at(TT.Eof)) {
          body.push(parseStmt());
        }
        cases.push({ value: null, body });
      } else {
        advance(); // skip unexpected token
      }
    }
    expect(TT.RBrace);
    return { kind: 'switch', expr, cases };
  }

  // ── Expression Parsing (Pratt-style precedence climbing) ──

  function parseExpr(): Expr {
    return parseAssignment();
  }

  function parseAssignment(): Expr {
    const left = parseTernary();
    if (at(TT.Eq) || at(TT.PlusEq) || at(TT.MinusEq) || at(TT.StarEq) || at(TT.SlashEq)) {
      const op = advance().value;
      const right = parseAssignment(); // right-associative
      return { kind: 'assign', target: left, op, value: right };
    }
    return left;
  }

  function parseTernary(): Expr {
    let expr = parseOr();
    if (match(TT.Question)) {
      const then = parseExpr();
      expect(TT.Colon);
      const else_ = parseTernary();
      expr = { kind: 'ternary', cond: expr, then, else_ };
    }
    return expr;
  }

  function parseOr(): Expr {
    let left = parseAnd();
    while (match(TT.OrOr)) {
      left = { kind: 'binary', op: '||', left, right: parseAnd() };
    }
    return left;
  }

  function parseAnd(): Expr {
    let left = parseBitOr();
    while (match(TT.AndAnd)) {
      left = { kind: 'binary', op: '&&', left, right: parseBitOr() };
    }
    return left;
  }

  function parseBitOr(): Expr {
    let left = parseBitXor();
    while (match(TT.Pipe)) {
      left = { kind: 'binary', op: '|', left, right: parseBitXor() };
    }
    return left;
  }

  function parseBitXor(): Expr {
    let left = parseBitAnd();
    while (match(TT.Caret)) {
      left = { kind: 'binary', op: '^', left, right: parseBitAnd() };
    }
    return left;
  }

  function parseBitAnd(): Expr {
    let left = parseEquality();
    while (match(TT.Amp)) {
      left = { kind: 'binary', op: '&', left, right: parseEquality() };
    }
    return left;
  }

  function parseEquality(): Expr {
    let left = parseComparison();
    while (at(TT.EqEq) || at(TT.NEq)) {
      const op = advance().value;
      left = { kind: 'binary', op, left, right: parseComparison() };
    }
    return left;
  }

  function parseComparison(): Expr {
    let left = parseShift();
    while (at(TT.Lt) || at(TT.Gt) || at(TT.LEq) || at(TT.GEq)) {
      const op = advance().value;
      left = { kind: 'binary', op, left, right: parseShift() };
    }
    return left;
  }

  function parseShift(): Expr {
    let left = parseAdditive();
    while (at(TT.Shl) || at(TT.Shr)) {
      const op = advance().value;
      left = { kind: 'binary', op, left, right: parseAdditive() };
    }
    return left;
  }

  function parseAdditive(): Expr {
    let left = parseMultiplicative();
    while (at(TT.Plus) || at(TT.Minus)) {
      const op = advance().value;
      left = { kind: 'binary', op, left, right: parseMultiplicative() };
    }
    return left;
  }

  function parseMultiplicative(): Expr {
    let left = parseUnary();
    while (at(TT.Star) || at(TT.Slash) || at(TT.Percent)) {
      const op = advance().value;
      left = { kind: 'binary', op, left, right: parseUnary() };
    }
    return left;
  }

  function parseUnary(): Expr {
    if (at(TT.Minus) || at(TT.Bang) || at(TT.Tilde)) {
      const op = advance().value;
      return { kind: 'unary', op, operand: parseUnary(), prefix: true };
    }
    if (at(TT.PlusPlus) || at(TT.MinusMinus)) {
      const op = advance().value;
      return { kind: 'unary', op, operand: parseUnary(), prefix: true };
    }
    return parsePostfix();
  }

  function parsePostfix(): Expr {
    let expr = parsePrimary();
    while (true) {
      if (at(TT.PlusPlus) || at(TT.MinusMinus)) {
        const op = advance().value;
        expr = { kind: 'unary', op, operand: expr, prefix: false };
      } else if (match(TT.Dot)) {
        const field = expect(TT.Ident).value;
        expr = { kind: 'member', object: expr, field };
      } else if (at(TT.LBracket)) {
        advance();
        const index = parseExpr();
        expect(TT.RBracket);
        expr = { kind: 'index', object: expr, index };
      } else {
        break;
      }
    }
    return expr;
  }

  function parsePrimary(): Expr {
    const t = peek();

    // Integer literal
    if (t.type === TT.IntLit) {
      advance();
      return { kind: 'int_lit', value: parseInt(t.value, 10) };
    }
    // Hex literal
    if (t.type === TT.HexLit) {
      advance();
      return { kind: 'int_lit', value: parseInt(t.value, 16) };
    }
    // Float literal
    if (t.type === TT.FloatLit) {
      advance();
      return { kind: 'float_lit', value: parseFloat(t.value) };
    }
    // String literal
    if (t.type === TT.StringLit || t.type === TT.RawStringLit) {
      advance();
      return { kind: 'string_lit', value: t.value };
    }
    // Boolean literals
    if (t.type === TT.KwTrue) { advance(); return { kind: 'bool_lit', value: true }; }
    if (t.type === TT.KwFalse) { advance(); return { kind: 'bool_lit', value: false }; }

    // Vector literal: [x, y, z]
    if (t.type === TT.LBracket) {
      advance();
      const x = parseExpr();
      expect(TT.Comma);
      const y = parseExpr();
      expect(TT.Comma);
      const z = parseExpr();
      expect(TT.RBracket);
      return { kind: 'vector_lit', x, y, z };
    }

    // Parenthesized expression
    if (t.type === TT.LParen) {
      advance();
      const expr = parseExpr();
      expect(TT.RParen);
      return expr;
    }

    // Identifier or function call
    if (t.type === TT.Ident) {
      const name = advance().value;

      // Special: OBJECT_INVALID, OBJECT_SELF
      if (name === 'OBJECT_INVALID' || name === 'OBJECT_SELF') {
        return { kind: 'object_invalid' };
      }

      // Function call
      if (at(TT.LParen)) {
        advance();
        const args: Expr[] = [];
        if (!at(TT.RParen)) {
          do {
            args.push(parseExpr());
          } while (match(TT.Comma));
        }
        expect(TT.RParen);
        return { kind: 'call', callee: name, args };
      }

      return { kind: 'ident', name };
    }

    // Unrecognized — return a placeholder
    advance();
    return { kind: 'int_lit', value: 0 };
  }

  return parseProgram();
}
