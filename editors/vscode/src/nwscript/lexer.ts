import { TT, Token } from './types';

const KEYWORDS: Record<string, TT> = {
  'int': TT.KwInt, 'float': TT.KwFloat, 'string': TT.KwString,
  'void': TT.KwVoid, 'object': TT.KwObject, 'json': TT.KwJson,
  'struct': TT.KwStruct, 'vector': TT.KwVector, 'effect': TT.KwEffect,
  'itemproperty': TT.KwItemProperty, 'action': TT.KwAction,
  'location': TT.KwLocation, 'talent': TT.KwTalent, 'sqlquery': TT.KwSqlQuery,
  'cassowary': TT.KwCassowary,
  'if': TT.KwIf, 'else': TT.KwElse, 'for': TT.KwFor, 'while': TT.KwWhile,
  'do': TT.KwDo, 'switch': TT.KwSwitch, 'case': TT.KwCase, 'default': TT.KwDefault,
  'break': TT.KwBreak, 'continue': TT.KwContinue, 'return': TT.KwReturn,
  'const': TT.KwConst,
  'TRUE': TT.KwTrue, 'FALSE': TT.KwFalse,
  'OBJECT_INVALID': TT.Ident, 'OBJECT_SELF': TT.Ident,
};

const TYPE_KEYWORDS = new Set([
  TT.KwInt, TT.KwFloat, TT.KwString, TT.KwVoid, TT.KwObject, TT.KwJson,
  TT.KwStruct, TT.KwVector, TT.KwEffect, TT.KwItemProperty, TT.KwAction,
  TT.KwLocation, TT.KwTalent, TT.KwSqlQuery, TT.KwCassowary,
]);

export function isTypeKeyword(tt: TT): boolean {
  return TYPE_KEYWORDS.has(tt);
}

export function lex(src: string): Token[] {
  const tokens: Token[] = [];
  let pos = 0, line = 1, col = 1;

  function peek(offset = 0): string { return src[pos + offset] ?? '\0'; }
  function advance(): string {
    const ch = src[pos++] ?? '\0';
    if (ch === '\n') { line++; col = 1; } else { col++; }
    return ch;
  }
  function emit(type: TT, value: string, l: number, c: number) {
    tokens.push({ type, value, line: l, col: c });
  }

  while (pos < src.length) {
    const startLine = line, startCol = col;
    const ch = peek();

    // Whitespace
    if (ch === ' ' || ch === '\t' || ch === '\r' || ch === '\n') {
      advance();
      continue;
    }

    // Line comment
    if (ch === '/' && peek(1) === '/') {
      while (pos < src.length && peek() !== '\n') advance();
      continue;
    }

    // Block comment
    if (ch === '/' && peek(1) === '*') {
      advance(); advance();
      while (pos < src.length) {
        if (peek() === '*' && peek(1) === '/') { advance(); advance(); break; }
        advance();
      }
      continue;
    }

    // #include or #
    if (ch === '#') {
      advance();
      // Check for 'include'
      let word = '';
      while (pos < src.length && /[a-zA-Z_]/.test(peek())) {
        word += advance();
      }
      if (word === 'include') {
        // Skip whitespace
        while (pos < src.length && (peek() === ' ' || peek() === '\t')) advance();
        // Read the path (either "path" or <path>)
        let path = '';
        const delim = advance(); // " or <
        const endDelim = delim === '<' ? '>' : delim;
        while (pos < src.length && peek() !== endDelim && peek() !== '\n') {
          path += advance();
        }
        if (peek() === endDelim) advance();
        emit(TT.KwInclude, path, startLine, startCol);
        continue;
      }
      emit(TT.Hash, '#' + word, startLine, startCol);
      continue;
    }

    // Raw string literal r"..."
    if (ch === 'r' && peek(1) === '"') {
      advance(); // r
      advance(); // opening "
      let str = '';
      while (pos < src.length && peek() !== '"') {
        if (peek() === '"' && peek(1) === '"') {
          str += '"';
          advance(); advance();
        } else {
          str += advance();
        }
      }
      if (peek() === '"') advance();
      emit(TT.RawStringLit, str, startLine, startCol);
      continue;
    }

    // String literal
    if (ch === '"') {
      advance();
      let str = '';
      while (pos < src.length && peek() !== '"') {
        if (peek() === '\\') {
          advance();
          const esc = advance();
          switch (esc) {
            case 'n': str += '\n'; break;
            case 't': str += '\t'; break;
            case '\\': str += '\\'; break;
            case '"': str += '"'; break;
            default: str += esc;
          }
        } else {
          str += advance();
        }
      }
      if (peek() === '"') advance();
      emit(TT.StringLit, str, startLine, startCol);
      continue;
    }

    // Numbers
    if (/[0-9]/.test(ch) || (ch === '.' && /[0-9]/.test(peek(1)))) {
      let num = '';
      // Hex
      if (ch === '0' && (peek(1) === 'x' || peek(1) === 'X')) {
        num += advance(); num += advance();
        while (/[0-9a-fA-F]/.test(peek())) num += advance();
        emit(TT.HexLit, num, startLine, startCol);
        continue;
      }
      let isFloat = false;
      while (/[0-9]/.test(peek())) num += advance();
      if (peek() === '.' && /[0-9]/.test(peek(1))) {
        isFloat = true;
        num += advance(); // .
        while (/[0-9]/.test(peek())) num += advance();
      }
      if (peek() === 'f' || peek() === 'F') {
        isFloat = true;
        advance(); // consume 'f'
      }
      emit(isFloat ? TT.FloatLit : TT.IntLit, num, startLine, startCol);
      continue;
    }

    // Identifiers and keywords
    if (/[a-zA-Z_]/.test(ch)) {
      let id = '';
      while (/[a-zA-Z0-9_]/.test(peek())) id += advance();
      const kw = KEYWORDS[id];
      emit(kw !== undefined ? kw : TT.Ident, id, startLine, startCol);
      continue;
    }

    // Two-char operators
    const two = ch + peek(1);
    const twoMap: Record<string, TT> = {
      '==': TT.EqEq, '!=': TT.NEq, '<=': TT.LEq, '>=': TT.GEq,
      '&&': TT.AndAnd, '||': TT.OrOr, '<<': TT.Shl, '>>': TT.Shr,
      '+=': TT.PlusEq, '-=': TT.MinusEq, '*=': TT.StarEq, '/=': TT.SlashEq,
      '++': TT.PlusPlus, '--': TT.MinusMinus, '::': TT.ColonColon,
    };
    if (twoMap[two]) {
      advance(); advance();
      emit(twoMap[two], two, startLine, startCol);
      continue;
    }

    // Single-char operators/punctuation
    const oneMap: Record<string, TT> = {
      '+': TT.Plus, '-': TT.Minus, '*': TT.Star, '/': TT.Slash, '%': TT.Percent,
      '<': TT.Lt, '>': TT.Gt, '=': TT.Eq, '!': TT.Bang,
      '&': TT.Amp, '|': TT.Pipe, '^': TT.Caret, '~': TT.Tilde,
      '?': TT.Question, ':': TT.Colon,
      '(': TT.LParen, ')': TT.RParen, '{': TT.LBrace, '}': TT.RBrace,
      '[': TT.LBracket, ']': TT.RBracket,
      ';': TT.Semi, ',': TT.Comma, '.': TT.Dot,
    };
    if (oneMap[ch]) {
      advance();
      emit(oneMap[ch], ch, startLine, startCol);
      continue;
    }

    // Unknown character — skip
    advance();
  }

  emit(TT.Eof, '', line, col);
  return tokens;
}
