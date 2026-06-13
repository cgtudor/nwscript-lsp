import { Expr, Stmt, Program, Param, Value, BreakSignal, ContinueSignal, ReturnSignal, OBJECT_INVALID } from './types';
import { lex } from './lexer';
import { parse } from './parser';
import { NUI_CONSTANTS, JSON_CONSTANTS, jsonBuiltins, nuiBuiltins, engineMocks } from './nui-builtins';
import { FrameworkBuilder } from './framework-builder';

// ── Environment (scoped variable storage) ────────────────────

class Env {
  private vars = new Map<string, Value>();
  constructor(private parent: Env | null = null) { }

  get(name: string): Value {
    if (this.vars.has(name)) return this.vars.get(name)!;
    if (this.parent) return this.parent.get(name);
    return undefined;
  }

  set(name: string, value: Value): void {
    // Walk up to find existing binding
    if (this.vars.has(name)) { this.vars.set(name, value); return; }
    if (this.parent && this.parent.has(name)) { this.parent.set(name, value); return; }
    // New binding in current scope
    this.vars.set(name, value);
  }

  define(name: string, value: Value): void {
    this.vars.set(name, value);
  }

  has(name: string): boolean {
    if (this.vars.has(name)) return true;
    if (this.parent) return this.parent.has(name);
    return false;
  }

  child(): Env { return new Env(this); }
}

// ── Interpreter ──────────────────────────────────────────────

const MAX_ITERATIONS = 100_000;
const MAX_CALL_DEPTH = 100;

class Interpreter {
  private globals: Env;
  private functions = new Map<string, { params: Param[]; body: Stmt[] }>();
  private builtins = new Map<string, (...args: any[]) => any>();
  private callDepth = 0;
  errors: string[] = [];
  private windowJson: Value = null;
  private windowGeometry: { x: number; y: number; w: number; h: number } | null = null;
  private lastGroupId: string | null = null;  // last group ID used in NuiSetGroupLayout
  // When set, the TDN nui_i_library framework is in use: its private engine helpers
  // (nui_SetObject, nui_IncrementPath, ...) are routed here instead of running their
  // SQLite-backed NWScript bodies. Null for vanilla nw_inc_nui.nss forms.
  private framework: FrameworkBuilder | null = null;

  constructor() {
    this.globals = new Env();
    this.registerBuiltins();
  }

  private registerBuiltins() {
    // NUI constants (needed before includes are evaluated)
    for (const [k, v] of Object.entries(NUI_CONSTANTS)) this.globals.define(k, v);
    for (const [k, v] of Object.entries(JSON_CONSTANTS)) this.globals.define(k, v);
    this.globals.define('TRUE', 1);
    this.globals.define('FALSE', 0);
    this.globals.define('OBJECT_INVALID', OBJECT_INVALID);
    this.globals.define('OBJECT_SELF', OBJECT_INVALID);

    // JSON engine functions — native (no NWScript body), must be builtins
    for (const [k, v] of Object.entries(jsonBuiltins)) this.builtins.set(k, v);

    // Engine mocks — native functions whose return values affect control flow
    // (GetLocalJson must return null, GetIsDM must return 1, etc.)
    for (const [k, v] of Object.entries(engineMocks)) this.builtins.set(k, v);

    // NUI functions as fallback — if include resolver loads nw_inc_nui.nss,
    // the source definitions take precedence (user funcs checked first in dispatch).
    // If not, these builtins ensure NUI code still works.
    for (const [k, v] of Object.entries(nuiBuiltins)) this.builtins.set(k, v);

    // NUI interception is now handled directly in the call dispatch (evalExpr 'call' case)
    // No need for builtin overrides — the dispatch intercepts NuiCreate, NuiWindow,
    // NuiSetBind, NuiFindWindow before checking builtins or user functions.

    // Type conversions — native engine functions
    this.builtins.set('IntToString', (n: any) => String(Math.trunc(Number(n) || 0)));
    this.builtins.set('FloatToString', (f: any, w: number = 0, d: number = 3) => {
      const v = Number(f) || 0.0;
      return w > 0 ? v.toFixed(d).padStart(w) : v.toFixed(d);
    });
    this.builtins.set('StringToInt', (s: any) => { const n = parseInt(String(s), 10); return isNaN(n) ? 0 : n; });
    this.builtins.set('StringToFloat', (s: any) => { const n = parseFloat(String(s)); return isNaN(n) ? 0.0 : n; });
    this.builtins.set('IntToFloat', (n: any) => Number(n) || 0.0);
    this.builtins.set('FloatToInt', (f: any) => Math.trunc(Number(f) || 0));

    // String functions — native
    this.builtins.set('GetStringLength', (s: any) => String(s ?? '').length);
    this.builtins.set('GetStringLeft', (s: any, n: number) => String(s ?? '').substring(0, n));
    this.builtins.set('GetStringRight', (s: any, n: number) => { const str = String(s ?? ''); return str.substring(str.length - n); });
    this.builtins.set('GetSubString', (s: any, start: number, count: number) => String(s ?? '').substring(start, start + count));
    this.builtins.set('GetStringLowerCase', (s: any) => String(s ?? '').toLowerCase());
    this.builtins.set('GetStringUpperCase', (s: any) => String(s ?? '').toUpperCase());
    this.builtins.set('FindSubString', (s: any, sub: any, start: number = 0) => String(s ?? '').indexOf(String(sub ?? ''), start));

    // Math — native
    this.builtins.set('abs', (n: number) => Math.abs(n));
    this.builtins.set('fabs', (n: number) => Math.abs(n));
    this.builtins.set('pow', (a: number, b: number) => Math.pow(a, b));
    this.builtins.set('sqrt', (n: number) => Math.sqrt(n));
    this.builtins.set('log', (n: number) => Math.log(n));
    this.builtins.set('cos', (n: number) => Math.cos(n));
    this.builtins.set('sin', (n: number) => Math.sin(n));
    this.builtins.set('tan', (n: number) => Math.tan(n));
    this.builtins.set('acos', (n: number) => Math.acos(n));
    this.builtins.set('asin', (n: number) => Math.asin(n));
    this.builtins.set('atan', (n: number) => Math.atan(n));
    this.builtins.set('Random', (n: number) => 0);

    // TLK / 2DA lookups — native, return placeholder values
    this.builtins.set('GetStringByStrRef', (n: number) => `[strref:${n}]`);
    this.builtins.set('Get2DAString', (s2da: string, col: string, row: number) => '');
    this.builtins.set('GetPlayerDeviceProperty', () => 1920);

    // No-op stubs for side-effect functions
    this.builtins.set('PrintString', () => {});
    this.builtins.set('SendMessageToPC', () => {});
    this.builtins.set('SendMessageToAllDMs', () => {});
    this.builtins.set('WriteTimestampedLogEntry', () => {});
    this.builtins.set('DelayCommand', () => {});
    this.builtins.set('AssignCommand', () => {});
    this.builtins.set('ActionDoCommand', () => {});
    this.builtins.set('EnterTargetingMode', () => {});

    // NuiFindWindow — return 0 so "if already open" guards pass
    this.builtins.set('NuiFindWindow', () => 0);
    this.builtins.set('NuiSetBind', () => {});
    this.builtins.set('NuiSetBindWatch', () => {});
    this.builtins.set('NuiSetGroupLayout', () => {});
    this.builtins.set('NuiDestroy', () => {});
  }

  run(program: Program): Value {
    // First pass: collect function declarations and top-level constants/vars
    for (const stmt of program.stmts) {
      if (stmt.kind === 'func_decl' && stmt.body.length > 0) {
        this.functions.set(stmt.name, { params: stmt.params, body: stmt.body });
      } else if (stmt.kind === 'const_decl' || stmt.kind === 'var_decl') {
        this.execStmt(stmt, this.globals);
      } else if (stmt.kind === 'struct_decl') {
        // Store struct definition for later instantiation
        this.globals.define('__struct_' + stmt.name, stmt.fields as any);
      } else if (stmt.kind === 'include') {
        // Skip includes — builtins are already registered
      }
    }
    return this.windowJson;
  }

  callFunction(name: string, args: Value[] = []): Value {
    const func = this.functions.get(name);
    if (!func) {
      this.errors.push(`Function '${name}' not found`);
      return undefined;
    }

    if (this.callDepth >= MAX_CALL_DEPTH) {
      this.errors.push(`Maximum call depth exceeded calling '${name}'`);
      return undefined;
    }

    this.callDepth++;
    const env = this.globals.child();

    // Bind parameters
    for (let i = 0; i < func.params.length; i++) {
      const param = func.params[i];
      let value = args[i];
      if (value === undefined && param.default_) {
        value = this.evalExpr(param.default_, env);
      }
      if (value === undefined) value = this.defaultForType(param.type);
      env.define(param.name, value);
    }

    try {
      this.execBlock(func.body, env);
    } catch (e) {
      if (e instanceof ReturnSignal) {
        this.callDepth--;
        return e.value;
      }
      throw e;
    }
    this.callDepth--;
    return undefined;
  }

  private defaultForType(type: string): Value {
    if (type === 'int') return 0;
    if (type === 'float') return 0.0;
    if (type === 'string') return '';
    if (type === 'json') return null;
    if (type === 'object') return OBJECT_INVALID;
    if (type.startsWith('struct ')) {
      const structName = type.slice(7);
      const fields = this.globals.get('__struct_' + structName) as Param[] | undefined;
      const result: Record<string, Value> = {};
      if (fields && Array.isArray(fields)) {
        for (const f of fields) {
          result[f.name] = this.defaultForType(f.type);
        }
      }
      return { __struct: structName, fields: result };
    }
    return undefined;
  }

  private execBlock(stmts: Stmt[], env: Env): void {
    for (const stmt of stmts) {
      this.execStmt(stmt, env);
    }
  }

  private execStmt(stmt: Stmt, env: Env): void {
    switch (stmt.kind) {
      case 'var_decl': {
        let val = stmt.init ? this.evalExpr(stmt.init, env) : this.defaultForType(stmt.type);
        env.define(stmt.name, val);
        break;
      }
      case 'const_decl': {
        const val = this.evalExpr(stmt.init, env);
        env.define(stmt.name, val);
        break;
      }
      case 'expr_stmt':
        this.evalExpr(stmt.expr, env);
        break;
      case 'if': {
        const cond = this.truthy(this.evalExpr(stmt.cond, env));
        if (cond) {
          this.execBlock(stmt.then, env.child());
        } else if (stmt.else_) {
          this.execBlock(stmt.else_, env.child());
        }
        break;
      }
      case 'for': {
        const forEnv = env.child();
        if (stmt.init) this.execStmt(stmt.init, forEnv);
        let iterations = 0;
        while (true) {
          if (++iterations > MAX_ITERATIONS) {
            this.errors.push('Maximum loop iterations exceeded');
            break;
          }
          if (stmt.cond) {
            const c = this.evalExpr(stmt.cond, forEnv);
            if (!this.truthy(c)) break;
          }
          try {
            this.execBlock(stmt.body, forEnv.child());
          } catch (e) {
            if (e instanceof BreakSignal) break;
            if (e instanceof ContinueSignal) { /* continue */ }
            else throw e;
          }
          if (stmt.incr) this.evalExpr(stmt.incr, forEnv);
        }
        break;
      }
      case 'while': {
        let iterations = 0;
        while (true) {
          if (++iterations > MAX_ITERATIONS) {
            this.errors.push('Maximum loop iterations exceeded');
            break;
          }
          if (!this.truthy(this.evalExpr(stmt.cond, env))) break;
          try {
            this.execBlock(stmt.body, env.child());
          } catch (e) {
            if (e instanceof BreakSignal) break;
            if (e instanceof ContinueSignal) continue;
            throw e;
          }
        }
        break;
      }
      case 'do_while': {
        let iterations = 0;
        do {
          if (++iterations > MAX_ITERATIONS) {
            this.errors.push('Maximum loop iterations exceeded');
            break;
          }
          try {
            this.execBlock(stmt.body, env.child());
          } catch (e) {
            if (e instanceof BreakSignal) break;
            if (e instanceof ContinueSignal) continue;
            throw e;
          }
        } while (this.truthy(this.evalExpr(stmt.cond, env)));
        break;
      }
      case 'switch': {
        const val = this.evalExpr(stmt.expr, env);
        let matched = false;
        let fell = false;
        for (const c of stmt.cases) {
          if (!fell && !matched) {
            if (c.value === null) { matched = true; } // default
            else {
              const caseVal = this.evalExpr(c.value, env);
              if (val === caseVal) matched = true;
            }
          }
          if (matched || fell) {
            try {
              this.execBlock(c.body, env.child());
              fell = true; // fall-through
            } catch (e) {
              if (e instanceof BreakSignal) return;
              throw e;
            }
          }
        }
        break;
      }
      case 'return':
        throw new ReturnSignal(stmt.value ? this.evalExpr(stmt.value, env) : undefined);
      case 'break':
        throw new BreakSignal();
      case 'continue':
        throw new ContinueSignal();
      case 'block':
        this.execBlock(stmt.stmts, env.child());
        break;
      case 'func_decl':
        if (stmt.body.length > 0) {
          this.functions.set(stmt.name, { params: stmt.params, body: stmt.body });
        }
        break;
      case 'struct_decl':
        this.globals.define('__struct_' + stmt.name, stmt.fields as any);
        break;
      case 'include':
        // Skip
        break;
    }
  }

  evalExpr(expr: Expr, env: Env): Value {
    switch (expr.kind) {
      case 'int_lit': return expr.value;
      case 'float_lit': return expr.value;
      case 'string_lit': return expr.value;
      case 'bool_lit': return expr.value ? 1 : 0; // NWScript TRUE=1, FALSE=0
      case 'object_invalid': return OBJECT_INVALID;

      case 'ident': {
        const val = env.get(expr.name);
        if (val === undefined && !env.has(expr.name)) {
          // Unknown constant (likely from nwscript.nss like DAMAGE_TYPE_FIRE) — return 0 silently
          return 0;
        }
        return val;
      }

      case 'binary': return this.evalBinary(expr.op, expr.left, expr.right, env);

      case 'unary': {
        const operand = this.evalExpr(expr.operand, env);
        switch (expr.op) {
          case '-': return -(operand as number);
          case '!': return this.truthy(operand) ? 0 : 1;
          case '~': return ~(operand as number);
          case '++': {
            const newVal = (operand as number) + 1;
            if (expr.operand.kind === 'ident') env.set(expr.operand.name, newVal);
            return expr.prefix ? newVal : operand;
          }
          case '--': {
            const newVal = (operand as number) - 1;
            if (expr.operand.kind === 'ident') env.set(expr.operand.name, newVal);
            return expr.prefix ? newVal : operand;
          }
          default: return operand;
        }
      }

      case 'call': {
        const args = expr.args.map(a => this.evalExpr(a, env));

        // Intercepted functions — capture window JSON and geometry directly here
        if (expr.callee === 'NuiCreate') {
          if (args.length >= 2) this.windowJson = args[1];
          return 1;
        }
        if (expr.callee === 'NuiWindow') {
          // Use the builtin NuiWindow to build the JSON
          const bi = this.builtins.get('NuiWindow');
          if (bi) {
            const result = bi(...args);
            this.windowJson = result;
            return result;
          }
        }
        if (expr.callee === 'NuiFindWindow') {
          return 0;
        }
        if (expr.callee === 'NuiSetBind') {
          // Capture geometry directly in the dispatch (proven to execute)
          const jValue = args[3];
          if (jValue && typeof jValue === 'object' && !Array.isArray(jValue)
              && 'w' in jValue && 'h' in jValue
              && typeof jValue.w === 'number' && typeof jValue.h === 'number'
              && jValue.w > 50 && jValue.h > 50) {
            if (!this.windowGeometry) {
              this.windowGeometry = { x: Number(jValue.x) || 0, y: Number(jValue.y) || 0, w: Number(jValue.w), h: Number(jValue.h) };
            }
          }
          return; // NuiSetBind is void
        }
        if (expr.callee === 'NuiSetGroupLayout') {
          // NuiSetGroupLayout(oPC, nToken, sGroupId, jLayout) — update group child
          const groupId = args[2] as string;
          const newLayout = args[3];
          if (this.windowJson && groupId && newLayout) {
            this.applyGroupLayout(this.windowJson, groupId, newLayout);
            this.lastGroupId = groupId;
          }
          return;
        }
        if (expr.callee === 'NuiSetBindWatch' || expr.callee === 'NuiDestroy') {
          return; // void no-ops
        }

        // TDN framework engine interception: when the nui_i_library framework is
        // active, its private builder helpers are routed to the FrameworkBuilder
        // instead of running their SQLite-backed bodies. This must come before the
        // user-function dispatch so the included nui_i_main.nss bodies are bypassed.
        // The public NUI_* functions (NUI_AddColumn, NUI_BindLabel, ...) are NOT in
        // this table — their real bodies run and funnel into these private helpers.
        if (this.framework) {
          const fh = FRAMEWORK_HELPERS[expr.callee];
          if (fh) return fh(this.framework, args);
        }

        // Check user-defined functions FIRST (from included .nss files)
        // This lets nw_inc_nui.nss definitions override the fallback builtins
        const userFunc = this.functions.get(expr.callee);
        if (userFunc && userFunc.body.length > 0) {
          return this.callFunction(expr.callee, args);
        }

        // Then builtins (engine natives + NUI fallbacks)
        const builtin = this.builtins.get(expr.callee);
        if (builtin) {
          try {
            return builtin(...args);
          } catch (e: any) {
            this.errors.push(`Error in ${expr.callee}: ${e.message}`);
            return undefined;
          }
        }

        // Unknown function — native engine prototype with no NWScript body.
        // Silently return 0. This handles Effect*, NWNX*, Get*, etc.
        return 0;
      }

      case 'member': {
        const obj = this.evalExpr(expr.object, env);
        if (obj && typeof obj === 'object' && '__struct' in (obj as any)) {
          return (obj as any).fields[expr.field];
        }
        if (obj && typeof obj === 'object') {
          return (obj as Record<string, Value>)[expr.field];
        }
        return undefined;
      }

      case 'index': {
        const obj = this.evalExpr(expr.object, env);
        const idx = this.evalExpr(expr.index, env);
        if (Array.isArray(obj) && typeof idx === 'number') return obj[idx];
        return undefined;
      }

      case 'ternary': {
        return this.truthy(this.evalExpr(expr.cond, env))
          ? this.evalExpr(expr.then, env)
          : this.evalExpr(expr.else_, env);
      }

      case 'assign': {
        const val = this.evalExpr(expr.value, env);
        return this.assign(expr.target, expr.op, val, env);
      }

      case 'vector_lit': {
        const x = this.evalExpr(expr.x, env) as number;
        const y = this.evalExpr(expr.y, env) as number;
        const z = this.evalExpr(expr.z, env) as number;
        return { __vector: true as const, x, y, z };
      }
    }
  }

  private evalBinary(op: string, left: Expr, right: Expr, env: Env): Value {
    // Short-circuit for logical ops
    if (op === '&&') {
      return (this.truthy(this.evalExpr(left, env)) && this.truthy(this.evalExpr(right, env))) ? 1 : 0;
    }
    if (op === '||') {
      return (this.truthy(this.evalExpr(left, env)) || this.truthy(this.evalExpr(right, env))) ? 1 : 0;
    }

    const l = this.evalExpr(left, env);
    const r = this.evalExpr(right, env);

    // String concatenation
    if (op === '+' && (typeof l === 'string' || typeof r === 'string')) {
      return String(l ?? '') + String(r ?? '');
    }

    const ln = l as number;
    const rn = r as number;

    switch (op) {
      case '+': return ln + rn;
      case '-': return ln - rn;
      case '*': return ln * rn;
      case '/': return rn !== 0 ? (Number.isInteger(ln) && Number.isInteger(rn) ? Math.trunc(ln / rn) : ln / rn) : 0;
      case '%': return rn !== 0 ? ln % rn : 0;
      case '==': return l === r ? 1 : 0;
      case '!=': return l !== r ? 1 : 0;
      case '<': return ln < rn ? 1 : 0;
      case '>': return ln > rn ? 1 : 0;
      case '<=': return ln <= rn ? 1 : 0;
      case '>=': return ln >= rn ? 1 : 0;
      case '&': return ln & rn;
      case '|': return ln | rn;
      case '^': return ln ^ rn;
      case '<<': return ln << rn;
      case '>>': return ln >> rn;
      default: return 0;
    }
  }

  private assign(target: Expr, op: string, val: Value, env: Env): Value {
    if (target.kind === 'ident') {
      let final_ = val;
      if (op !== '=') {
        const current = env.get(target.name) as number;
        const v = val as number;
        switch (op) {
          case '+=':
            if (typeof current === 'string') { final_ = current + String(v); break; }
            final_ = current + v; break;
          case '-=': final_ = current - v; break;
          case '*=': final_ = current * v; break;
          case '/=': final_ = v !== 0 ? current / v : 0; break;
        }
      }
      env.set(target.name, final_);
      return final_;
    }
    if (target.kind === 'member') {
      const obj = this.evalExpr(target.object, env);
      if (obj && typeof obj === 'object' && '__struct' in (obj as any)) {
        (obj as any).fields[target.field] = val;
        return val;
      }
    }
    return val;
  }

  private truthy(v: Value): boolean {
    if (v === null || v === undefined || v === 0 || v === '' || v === false) return false;
    return true;
  }

  /** Recursively find a group element by ID and replace its children with the new layout */
  private applyGroupLayout(node: any, groupId: string, newLayout: any): boolean {
    if (!node || typeof node !== 'object') return false;

    // Check if this node is the target group
    if (node.id === groupId && node.type === 'group') {
      node.children = [newLayout];
      return true;
    }

    // Search in root element
    if (node.root) {
      if (this.applyGroupLayout(node.root, groupId, newLayout)) return true;
    }

    // Search in children array
    if (Array.isArray(node.children)) {
      for (const child of node.children) {
        if (this.applyGroupLayout(child, groupId, newLayout)) return true;
      }
    }

    return false;
  }

  /** Get all user-defined function names */
  getFunctionNames(): string[] {
    return Array.from(this.functions.keys());
  }

  /** Get function definition (params + body) */
  getFunctionDef(name: string): { params: Param[]; body: Stmt[] } | undefined {
    return this.functions.get(name);
  }

  /** Get the captured window JSON */
  getWindowJson(): Value {
    return this.windowJson;
  }

  /** Get the last group ID used in NuiSetGroupLayout */
  getLastGroupId(): string | null {
    return this.lastGroupId;
  }

  /** Try calling a function and return its result if it looks like NUI JSON */
  tryCallForLayout(funcName: string): any | null {
    const funcDef = this.functions.get(funcName);
    if (!funcDef) return null;
    // Build default args: OBJECT_INVALID for objects, 0/""/{} for others
    const args = funcDef.params.map((p: Param) => {
      if (p.type === 'object') return OBJECT_INVALID;
      if (p.type === 'int' || p.type === 'float') return 0;
      if (p.type === 'string') return '';
      if (p.type === 'json') return null;
      return 0;
    });
    // Probing is side-effecting: the probed function may itself call
    // NuiWindow/NuiCreate (some "view" candidates are actually full window
    // builders), which would clobber the already-captured main window. Snapshot
    // and restore the capture state so probing never corrupts it.
    const savedJson = this.windowJson;
    const savedGeometry = this.windowGeometry;
    const savedGroupId = this.lastGroupId;
    try {
      const result = this.callFunction(funcName, args);
      if (isNuiLayout(result)) return result;
    } catch (e: any) {
      if (e instanceof ReturnSignal && isNuiLayout((e as any).value)) return (e as any).value;
    } finally {
      this.windowJson = savedJson;
      this.windowGeometry = savedGeometry;
      this.lastGroupId = savedGroupId;
    }
    return null;
  }

  /** Apply a layout to the swap group and return the updated window JSON */
  applyViewToGroup(groupId: string, layout: any): any {
    if (this.windowJson) {
      this.applyGroupLayout(this.windowJson, groupId, layout);
    }
    return this.windowJson;
  }

  /** Get the captured window geometry (from NuiSetBind "geometry") */
  getWindowGeometry(): { x: number; y: number; w: number; h: number } | null {
    return this.windowGeometry;
  }

  /** Activate the TDN nui_i_library framework engine for this run. */
  setFramework(builder: FrameworkBuilder): void {
    this.framework = builder;
  }

  /** Read a numeric top-level global (e.g. FORM_WIDTH). Returns null if absent/non-numeric. */
  getGlobalNumber(name: string): number | null {
    if (!this.globals.has(name)) return null;
    const v = this.globals.get(name);
    return typeof v === 'number' && isFinite(v) ? v : null;
  }
}

// ── TDN framework private-helper interception table ──────────────────────────
// Maps the nui_i_main.nss private engine function names to FrameworkBuilder
// methods. Default-arg semantics mirror the NWScript prototypes (missing call
// args fall back to the prototype default).

const FRAMEWORK_HELPERS: Record<string, (b: FrameworkBuilder, args: Value[]) => Value> = {
  nui_SaveForm: (b, a) => { b.saveForm(String(a[0] ?? ''), String(a[1] ?? '')); return undefined; },
  nui_DeleteForm: (b, a) => { b.deleteForm(String(a[0] ?? '')); return undefined; },
  nui_GetForm: (b, a) => b.getForm(String(a[0] ?? '')),
  nui_GetDefinitionValue: (b, a) => b.getDefinitionValue(String(a[0] ?? ''), String(a[1] ?? '')),

  nui_SetObject: (b, a) => { b.setObject(String(a[0] ?? ''), String(a[1] ?? ''), String(a[2] ?? '')); return undefined; },
  nui_SetControl: (b, a) => { b.setObject('', String(a[0] ?? ''), String(a[1] ?? '')); return undefined; },
  nui_SetProperty: (b, a) => { b.setObject(String(a[0] ?? ''), String(a[1] ?? '')); return undefined; },

  nui_IncrementPath: (b, a) => b.incrementPath(String(a[0] ?? ''), !!a[1]),
  nui_DecrementPath: (b, a) => b.decrementPath(a.length ? (a[0] as number) : 1),
  nui_SubstitutePath: (b, a) => b.substitutePath(String(a[0] ?? '')),
  nui_GetSubstitutedPath: (b, a) => b.getSubstitutedPath(String(a[0] ?? '')),
  nui_GetGroupKey: (b) => b.getGroupKey(),

  nui_SetPath: (b, a) => b.setPath(String(a[0] ?? '')),
  nui_GetPath: (b) => b.getPath(),
  nui_ResetPath: (b) => { b.resetPath(); return undefined; },

  nui_ToggleIncrementFlag: (b, a) => b.toggleIncrementFlag(a.length ? (a[0] as number) : -1),
  nui_GetIncrementFlag: (b) => b.getIncrementFlag(),
  nui_ToggleDrawlistFlag: (b, a) => b.toggleDrawlistFlag(a.length ? (a[0] as number) : -1),
  nui_GetDrawlistFlag: (b) => b.getDrawlistFlag(),
  nui_ToggleListboxFlag: (b, a) => b.toggleListboxFlag(a.length ? (a[0] as number) : -1),
  nui_GetListboxFlag: (b) => b.getListboxFlag(),
  nui_ToggleDefinitionFlag: (b, a) => b.toggleDefinitionFlag(a.length ? (a[0] as number) : -1),
  nui_GetDefinitionFlag: (b) => b.getDefinitionFlag(),

  nui_SetControlType: (b, a) => { b.setControlType(String(a[0] ?? '')); return undefined; },
  nui_GetControlType: (b) => b.getControlType(),

  nui_GetEntryCount: (b) => b.getEntryCount(),
  nui_ResetEntryCount: (b) => { b.resetEntryCount(); return undefined; },
  nui_IncrementEntryCount: (b, a) => b.incrementEntryCount(a.length ? (a[0] as number) : 1),

  nui_SetFormID: (b, a) => { b.setFormId(String(a[0] ?? '')); return undefined; },
  nui_GetFormID: (b) => b.getFormId(),
  nui_SetFormfile: (b, a) => { b.setFormfile(String(a[0] ?? '')); return undefined; },
  nui_GetFormfile: (b) => b.getFormfile(),

  nui_ClearVariables: (b) => { b.clearVariables(); return undefined; },
};

// ── AST Scanning ─────────────────────────────────────────────

/** Find functions whose body contains NuiWindow or NuiCreate calls. */
function findNuiFunctions(program: Program): string[] {
  const hits: string[] = [];
  for (const stmt of program.stmts) {
    if (stmt.kind === 'func_decl' && stmt.body.length > 0) {
      // Skip the framework's own API (NUI_DisplayForm/NUI_CreateForm/... from
      // nui_i_main.nss). These contain NuiCreate but are never the user's form
      // builder; a vanilla form may transitively include the framework, and we
      // must not mistake NUI_DisplayForm for its builder.
      if (/^(NUI_|nui_)/.test(stmt.name)) continue;
      if (astContainsCall(stmt.body, ['NuiWindow', 'NuiCreate'])) {
        hits.push(stmt.name);
      }
    }
  }
  return hits;
}

/**
 * Detect a TDN nui_i_library framework form: a DefineForm() whose body calls
 * NUI_CreateForm. Returns the entry function name ("DefineForm") or null.
 */
function findFrameworkEntry(program: Program): string | null {
  for (const stmt of program.stmts) {
    if (stmt.kind === 'func_decl' && stmt.name === 'DefineForm' && stmt.body.length > 0) {
      if (astContainsCall(stmt.body, ['NUI_CreateForm'])) return 'DefineForm';
    }
  }
  return null;
}

/**
 * Determine the builder/entry function defined *in a single file's own source*
 * (before includes are expanded). Used so the preview targets the form in the
 * file the user is editing, not a builder pulled in transitively from an include
 * (e.g. opening pc_edit_nui.nss must not render a "SubSpell" window defined in a
 * deeply-included file). Returns the framework DefineForm, else the first
 * non-framework NuiWindow/NuiCreate function, else null.
 */
export function findEntryFunction(source: string): string | null {
  let program: Program;
  try {
    program = parse(lex(source));
  } catch {
    return null;
  }
  const fw = findFrameworkEntry(program);
  if (fw) return fw;
  const nui = findNuiFunctions(program);
  return nui.length > 0 ? nui[0] : null;
}

/** Check if a value looks like a NUI layout element (col, row, group, etc.) */
function isNuiLayout(v: any): boolean {
  if (!v || typeof v !== 'object') return false;
  const t = v.type;
  return t === 'col' || t === 'row' || t === 'group' || t === 'list' || t === 'spacer';
}

/** Find the first group element with an id in a NUI JSON tree */
function findGroupIdInJson(json: any): string | null {
  if (!json || typeof json !== 'object') return null;
  if (json.type === 'group' && json.id) return json.id;
  if (json.root) { const r = findGroupIdInJson(json.root); if (r) return r; }
  if (Array.isArray(json.children)) {
    for (const c of json.children) { const r = findGroupIdInJson(c); if (r) return r; }
  }
  return null;
}

function astContainsCall(node: any, names: string[]): boolean {
  if (node == null || typeof node !== 'object') return false;
  if (node.kind === 'call' && names.includes(node.callee)) return true;
  if (Array.isArray(node)) return node.some(n => astContainsCall(n, names));
  for (const key of Object.keys(node)) {
    const v = node[key];
    if (typeof v === 'object' && v !== null && astContainsCall(v, names)) return true;
  }
  return false;
}

// ── Public API ───────────────────────────────────────────────

export interface EvalResult {
  json: any;
  errors: string[];
  functions: string[];
  geometry: { x: number; y: number; w: number; h: number } | null;
  /** View-builder functions that can be swapped into the group layout */
  views: string[];
  /** The group ID used for layout swapping (from NuiSetGroupLayout) */
  swapGroupId: string | null;
  /** The interpreter instance for re-calling view functions */
  interpreter: any;
}

/**
 * Parse and evaluate NWScript NUI code, returning the generated window JSON.
 *
 * @param source - NWScript source code
 * @param functionName - Optional function name to call (if omitted, tries to auto-detect)
 */
export function evaluateNuiScript(source: string, functionName?: string): EvalResult {
  const errors: string[] = [];

  // Lex
  let tokens;
  try {
    tokens = lex(source);
  } catch (e: any) {
    return { json: null, errors: [`Lexer error: ${e.message}`], functions: [], geometry: null, views: [], swapGroupId: null, interpreter: null };
  }

  // Parse
  let program;
  try {
    program = parse(tokens);
  } catch (e: any) {
    return { json: null, errors: [`Parse error: ${e.message}`], functions: [], geometry: null, views: [], swapGroupId: null, interpreter: null };
  }

  // Interpret
  const interp = new Interpreter();
  try {
    interp.run(program);
  } catch (e: any) {
    errors.push(`Runtime error during initialization: ${e.message}`);
  }

  const allFunctions = interp.getFunctionNames();

  // ── TDN nui_i_library framework path ──────────────────────────────────────
  // These forms build their window via stateful NUI_* builder calls (DefineForm)
  // backed by a JSON-path engine, not via NuiWindow/NuiCreate. Detect and route
  // through the FrameworkBuilder. (Vanilla forms fall through to the path below.)
  const frameworkEntry = findFrameworkEntry(program);
  if (frameworkEntry && (!functionName || functionName === frameworkEntry)) {
    const builder = new FrameworkBuilder();
    interp.setFramework(builder);
    try {
      interp.callFunction(frameworkEntry, []);
    } catch (e: any) {
      if (!(e instanceof ReturnSignal)) errors.push(`Runtime error in ${frameworkEntry}: ${e.message}`);
    }

    const windowJson = builder.getMainForm();

    // DefineForm only binds geometry (it's set at runtime in HandleNUIEvents).
    // Fall back to the form's declared FORM_WIDTH/FORM_HEIGHT consts if present.
    let geometry = interp.getWindowGeometry();
    if (!geometry) {
      const w = interp.getGlobalNumber('FORM_WIDTH');
      const h = interp.getGlobalNumber('FORM_HEIGHT');
      if (w && h && w > 50 && h > 50) geometry = { x: 0, y: 0, w, h };
    }

    const relevantErrors = interp.errors.filter(
      (e) => e && !e.startsWith('Unknown function:') && !e.startsWith('Unknown identifier:')
    );
    errors.push(...relevantErrors);

    return {
      json: windowJson,
      // A framework file builds exactly one form (its DefineForm). The dropdown is
      // informational (no re-eval handler), so list just the entry rather than the
      // flood of included nui_i_main.nss / util builder functions.
      errors: errors.filter((e) => e),
      functions: [frameworkEntry],
      geometry,
      views: [],
      swapGroupId: null,
      interpreter: interp,
    };
  }

  // Find functions that actually build NUI windows (contain NuiWindow/NuiCreate calls)
  const nuiFunctions = findNuiFunctions(program);

  // Determine which function to call
  let targetFunc = functionName;
  if (!targetFunc) {
    if (nuiFunctions.length > 0) {
      targetFunc = nuiFunctions[0];
    } else {
      // Fallback: name-based matching, excluding framework API (NUI_*/nui_*)
      targetFunc = allFunctions.find(f =>
        !/^(NUI_|nui_)/.test(f) && /nui|Nui|Open|Show|Create|Display/i.test(f)
      );
    }
  }

  if (targetFunc) {
    try {
      interp.callFunction(targetFunc, [OBJECT_INVALID]);
    } catch (e: any) {
      if (!(e instanceof ReturnSignal)) {
        errors.push(`Runtime error in ${targetFunc}: ${e.message}`);
      }
    }
  }

  // Filter out noise — only keep errors relevant to NUI evaluation
  const relevantErrors = interp.errors.filter(e =>
    e && !e.startsWith('Unknown function:') && !e.startsWith('Unknown identifier:')
  );
  errors.push(...relevantErrors);

  // Find view-builder functions: 0-param functions that return NUI layout JSON.
  // These can be swapped into a group via the Views dropdown.
  const views: string[] = [];
  // Detect swap group: either from NuiSetGroupLayout call, or from a NuiGroup with an id
  let groupId = interp.getLastGroupId();
  if (!groupId) {
    groupId = findGroupIdInJson(interp.getWindowJson());
  }
  if (groupId) {
    for (const funcName of allFunctions) {
      if (funcName === targetFunc) continue;
      // Skip NUI/JSON API wrappers and engine functions
      if (/^(Nui|Json|Get|Set|Send|Create|Destroy|Effect|Item|Action|Apply|Clear|Delay|Assign|Execute|Float|Int|String|Object|Print|Sql|Write|Read)/
          .test(funcName)) continue;
      const funcDef = interp.getFunctionDef(funcName);
      if (!funcDef) continue;
      const layout = interp.tryCallForLayout(funcName);
      if (layout) {
        views.push(funcName);
      }
    }
  }

  return {
    json: interp.getWindowJson(),
    errors: errors.filter(e => e),
    functions: nuiFunctions.length > 0 ? nuiFunctions : allFunctions,
    geometry: interp.getWindowGeometry(),
    views,
    swapGroupId: groupId,
    interpreter: interp,
  };
}
