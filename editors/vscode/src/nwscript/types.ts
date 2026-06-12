// ── Token Types ──────────────────────────────────────────────

export enum TT {
  // Literals
  IntLit, FloatLit, StringLit, HexLit, RawStringLit,
  // Keywords
  KwInt, KwFloat, KwString, KwVoid, KwObject, KwJson, KwStruct, KwVector,
  KwEffect, KwItemProperty, KwAction, KwLocation, KwTalent, KwSqlQuery, KwCassowary,
  KwIf, KwElse, KwFor, KwWhile, KwDo, KwSwitch, KwCase, KwDefault,
  KwBreak, KwContinue, KwReturn, KwConst,
  KwTrue, KwFalse,
  KwInclude,
  // Operators
  Plus, Minus, Star, Slash, Percent,
  EqEq, NEq, Lt, Gt, LEq, GEq,
  AndAnd, OrOr, Bang,
  Amp, Pipe, Caret, Tilde, Shl, Shr,
  Eq, PlusEq, MinusEq, StarEq, SlashEq,
  PlusPlus, MinusMinus,
  Question, Colon, ColonColon,
  // Delimiters
  LParen, RParen, LBrace, RBrace, LBracket, RBracket,
  Semi, Comma, Dot, Hash,
  // Other
  Ident, Eof,
}

export interface Token {
  type: TT;
  value: string;
  line: number;
  col: number;
}

// ── AST Node Types ───────────────────────────────────────────

export type Expr =
  | { kind: 'int_lit'; value: number }
  | { kind: 'float_lit'; value: number }
  | { kind: 'string_lit'; value: string }
  | { kind: 'bool_lit'; value: boolean }
  | { kind: 'ident'; name: string }
  | { kind: 'binary'; op: string; left: Expr; right: Expr }
  | { kind: 'unary'; op: string; operand: Expr; prefix: boolean }
  | { kind: 'call'; callee: string; args: Expr[] }
  | { kind: 'member'; object: Expr; field: string }
  | { kind: 'ternary'; cond: Expr; then: Expr; else_: Expr }
  | { kind: 'assign'; target: Expr; op: string; value: Expr }
  | { kind: 'index'; object: Expr; index: Expr }
  | { kind: 'vector_lit'; x: Expr; y: Expr; z: Expr }
  | { kind: 'object_invalid' };

export type Stmt =
  | { kind: 'var_decl'; type: string; name: string; init: Expr | null }
  | { kind: 'const_decl'; type: string; name: string; init: Expr }
  | { kind: 'expr_stmt'; expr: Expr }
  | { kind: 'if'; cond: Expr; then: Stmt[]; else_: Stmt[] | null }
  | { kind: 'for'; init: Stmt | null; cond: Expr | null; incr: Expr | null; body: Stmt[] }
  | { kind: 'while'; cond: Expr; body: Stmt[] }
  | { kind: 'do_while'; cond: Expr; body: Stmt[] }
  | { kind: 'switch'; expr: Expr; cases: SwitchCase[] }
  | { kind: 'return'; value: Expr | null }
  | { kind: 'break' }
  | { kind: 'continue' }
  | { kind: 'block'; stmts: Stmt[] }
  | { kind: 'func_decl'; retType: string; name: string; params: Param[]; body: Stmt[] }
  | { kind: 'struct_decl'; name: string; fields: Param[] }
  | { kind: 'include'; path: string };

export interface SwitchCase {
  value: Expr | null; // null = default
  body: Stmt[];
}

export interface Param {
  type: string;
  name: string;
  default_?: Expr;
}

export interface Program {
  stmts: Stmt[];
}

// ── Runtime Value Types ──────────────────────────────────────

export type Value = number | string | boolean | null | undefined
  | Value[] | { [key: string]: Value }
  | { __object: true; id: number }
  | { __struct: string; fields: Record<string, Value> }
  | { __vector: true; x: number; y: number; z: number };

export const OBJECT_INVALID = { __object: true as const, id: 0 };
export const OBJECT_SELF = { __object: true as const, id: -1 };

export class BreakSignal { }
export class ContinueSignal { }
export class ReturnSignal {
  constructor(public value: Value) { }
}
