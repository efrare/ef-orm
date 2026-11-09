# Arquitetura do ef-ORM

## Workspace

```
ef-orm/
├── Cargo.toml                 # [workspace] members + lints
├── crates/
│   ├── ef-orm-core/           # domínio puro: traits de entidade, Value, Error, metadados
│   ├── ef-orm-query/          # AST de query + trait Dialect + MssqlDialect
│   ├── ef-orm-driver/         # traits Driver/Conn/Tx/Row + pool próprio (sem TDS)
│   ├── ef-orm-driver-tds/     # adapter Tiberius (único dono do protocolo TDS)
│   ├── ef-orm/                # facade: DbContext, DbSet, ChangeTracker, UnitOfWork
│   ├── ef-orm-derive/         # proc-macros: #[derive(Entity)], #[derive(Model)]
│   ├── ef-orm-migrate/        # engine de migrations + tabela de histórico
│   └── ef-orm-cli/            # CLI: scaffold, migrate up/down, generate
└── examples/                  # exemplos executáveis (hello-crud, change-tracking…)
```

## Regra de dependência (seta `→` = depende de)

```
ef-orm-cli        → ef-orm, ef-orm-migrate
ef-orm-migrate    → ef-orm, ef-orm-query
ef-orm (facade)   → ef-orm-core, ef-orm-query, ef-orm-driver
ef-orm-driver-tds → ef-orm-driver, ef-orm-core
ef-orm-driver     → ef-orm-core
ef-orm-query      → ef-orm-core
ef-orm-core       → (nada — domínio puro)
ef-orm-derive     → (nada — gera código que usa a API de ef-orm)
```

Nada importa "para cima"; `core` não conhece SQL, drivers nem tokio; só
`driver-tds` importa `tiberius`; só `query` gera strings de SQL.

## Responsabilidades por crate

### ef-orm-core (zero dependências de infra)
- `Value` — enum de valores portáveis (Null, Bool, I64, F64, Str, Bytes, Uuid, DateTime, Decimal).
- Traits `Entity`, `Model`, `Column`, `PrimaryKey` (metadados: nome de tabela/coluna, tipo, nulabilidade).
- `OrmError` (thiserror) — erros de domínio: `NotFound`, `Conversion`, `Constraint`, `Transaction`…
- Nada de SQL, nada de tokio, nada de TDS.

### ef-orm-query
- AST: `SelectStatement`, `InsertStatement`, `UpdateStatement`, `DeleteStatement`,
  `Expr` (com grupos `And/Or`), `Order`, `Page`, `Ident`, `Table`.
- `trait Dialect` — `build(stmt) -> QueryResult<(String, Vec<Value>)>` + `quote_ident`,
  `placeholder` (1-based) e `default_schema`; capacidades (`supports_output_clause`, …).
- `MssqlDialect` — única implementação inicial (ver `mssql-dialect.md`).
- Regra: **o crate que gera SQL é só este**.

### ef-orm-driver
- `trait Driver { async fn connect(...) -> Conn }`, `trait Conn { fn execute, query, begin }`,
  `trait Tx { commit, rollback }`, `trait Row { get::<T>(idx|name) }`.
- Pool próprio mínimo: `Pool` com `Semaphore` de permitidos, `checkout` assíncrono, teste de
  conexão (`SELECT 1`), timeout de criação. Sem tokio nos tipos públicos (usar `async fn` + traits).
- Erros de infra estruturados (`DriverError`: connect, timeout, protocol, closed).

### ef-orm-driver-tds
- Único crate que importa `tiberius`. Converte `Value` ⇄ TDS (`IntoSql`/`FromSql`).
- Implementa `Conn`/`Tx`/`Row` sobre `tiberius::Client`.
- Parâmetros: o TDS usa `@P1, @P2…` — **a numeração é responsabilidade do dialeto** (ver dialect.md).

### ef-orm (facade — o "EF Core" do projeto)
- `DbContext` — sessão de trabalho: `DbSet<T>`, `save_changes()` (unit of work), `query::<T>()`.
- `DbSet<T>` — `insert`, `update`, `delete`, `find(pk)`, `select()` fluente, `as_no_tracking()`.
- `ChangeTracker` — snapshot por entidade: ao materializar, guarda os valores originais;
  em `save_changes`, calcula diff → gera UPDATE `WHERE pk = valor_original` (concorrência otimista).
- `UnitOfWork` — agrupa operações numa transação; `save_changes` = uma transação.

### ef-orm-derive
- `#[derive(Entity)]` em enum de colunas (estilo SeaORM `ColumnTrait`) + `#[derive(Model)]`
  em struct. Gera: nomes de tabela/colunas, `Column::def()`, mapeamento `Row → Model`.
- Nenhuma lógica de negócio nos macros — só geração de código que chama a API pública.

### ef-orm-migrate / ef-orm-cli
- Migrations são structs com `up(&mut Schema) -> Vec<Statement>` e `down(...)` usando a AST
  de `ef-orm-query` (nunca strings soltas). Tabela de histórico `__ef_migrations`.
- CLI (clap): `ef-orm-cli scaffold`, `migrate up|down|status`, `generate`.

## Fluxo de uma query (camada a camada)

```
usuario:  ctx.dbset::<User>().select().filter(|c| c.age.gt(18)).page(1, 20)
   │
   ▼ ef-orm
Select<User> monta SelectStatement (AST) com metadados do Entity
   │
   ▼ ef-orm-query
MssqlDialect::render → ("SELECT … OFFSET @P1 ROWS FETCH NEXT @P2 ROWS ONLY", [0, 20])
   │
   ▼ ef-orm-driver
Pool::checkout → Conn::query(sql, values)
   │
   ▼ ef-orm-driver-tds
tiberius::Query::bind… → TDS sobre TCP/TLS
   │
   ▼ ef-orm
Row → Model (via derive) → (opcional) ChangeTracker registra snapshot → itens ao usuário
```

## Salvar mudanças (change tracking)

```
save_changes():
 1. percorrer entradas do ChangeTracker (Inserted/Modified/Deleted)
 2. por entrada: gerar INSERT / UPDATE(diff) / DELETE via AST
 3. executar tudo numa única transação (Tx)
 4. commit → limpar snapshots → rollback em qualquer erro
```

UPDATE inclui no `WHERE` o valor **original** da PK (e de colunas com `concurrency_token`),
detectando sobreposição de escrita → erro `OrmError::Conflict`.
