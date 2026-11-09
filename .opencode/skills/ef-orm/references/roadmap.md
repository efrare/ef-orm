# Roadmap ef-ORM

Status: **fases 0–6 concluídas** (publicação no crates.io pendente de credenciais).
Legenda: `[ ]` pendente · `[~]` em andamento · `[x]` concluído

## Fase 0 — Fundação do workspace
- [x] Definir estratégia (do zero, MSSQL-first) e nome (`ef-orm`).
- [x] Criar skill do projeto com referências.
- [x] `cargo new --workspace`: crates `core`, `query`, `driver`, `driver-tds`, `ef-orm`,
      `derive`, `migrate`, `cli` com `Cargo.toml` de workspace (lints, perfis, versões).
- [x] `unsafe_code = "forbid"` (workspace lints) + CI básica (fmt, clippy, test) — GitHub Actions.
- [x] Licença MIT OR Apache-2.0 + `README.md` inicial.

## Fase 1 — Spike TDS (prova de vida) — **concluída**
> Nota: `Value` já existia no core (entregue na fase 2).
- [x] `ef-orm-driver`: traits `Driver`, `Conn`, `Tx`, `Row` + `DriverError`.
- [x] Pool mínimo (semáforo, `SELECT 1`, timeout).
- [x] `ef-orm-driver-tds`: `Config` (host/port/db/auth/TLS), conexão Tiberius, `execute`/`query`.
- [x] `Value` em `core` (entregue na fase 2: Null/Bool/Int/Float/Str/Bytes + `From` impls).
- [x] Conversão `Value ⇄ IntoSql/FromSql` no driver (bool, int, float, string, bytes).
- [x] Exemplo `examples/hello-raw.rs`: conectar, `SELECT 1`, imprimir.
- **Aceite**: exemplo roda contra SQL Server real (`192.168.0.10:1433`, credenciais
  via `EF_ORM_TEST_DB_*`) e devolve `SELECT 1 → 1` ✔
- **Suíte**: `fmt --check` + `clippy --all-targets -D warnings` + 57 testes verdes.
- Desvios/decisões anotados:
  - Traits desaguardados para `fn … -> impl Future<Output = …> + Send` (lint
    `async_fn_in_trait` reprovaria o CI; `Send` é o que o pool/spawn exigem).
  - `DriverError` ganhou a variante `Query` (erros do servidor) — fora do plano
    original de 4 variantes; o facade mapeia para `OrmError`.
  - Exemplo vive no pacote `examples/` (membro do workspace), não em `tests/`.
  - `Conn::query` materializa só o **primeiro result set**; `Value::Null` é
    bindado como `nvarchar` NULL; Guid/Numeric/DateTime → `DriverError::Protocol`
    até a fase 3.
  - Drop de `Tx` sem `commit`/`rollback` deixa a conexão em transação aberta
    (o `UnitOfWork` da fase 4 garante o pareamento).
  - Projeto-modelo seguido: `sql-orm-main` (`sql-orm-tiberius`) — `Debug`
    redigindo senha, mapeamento de erro por contexto, health check `SELECT 1 AS
    [health_check]`, bind via `Query::bind`.

## Fase 2 — Query engine — **concluída**
- [x] AST em `query`: `Select/Insert/Update/Delete`, `Expr`, `Order`, `Page`
      (agrupamentos `And/Or` vivem em `Expr`; `combine_filter` encadeia `and_where`).
- [x] `trait Dialect` (`build` → `QueryResult<(String, Vec<Value>)>`) + `MssqlDialect`:
      `@P1…`, quoting `[ ]` (escape `]]`), schema default `dbo`.
- [x] SELECT com WHERE/ORDER/OFFSET-FETCH/TOP (injeção de `ORDER BY (SELECT NULL)`);
      INSERT multi-row; UPDATE; DELETE.
- [x] `OUTPUT INSERTED.*` (INSERT/UPDATE) e `OUTPUT DELETED.*` (DELETE).
- [x] Testes snapshot: 22 em `tests/mssql_select.rs` + `tests/mssql_dml.rs`,
      mais 5 unitários (`Value`, `Expr`, `Table`).
- **Aceite**: cobertura dos 4 CRUDs + paginação, 100% testes snapshot verdes ✔
- Extras entregues: `Page` (1-based), `Expr::all/any`, `!expr` (`std::ops::Not`),
  `IN` vazio → `1 = 0`/`1 = 1`, validação de `INSERT` (colunas/linhas).

## Fase 3 — Entidades e CRUD — **concluída**
- [x] Traits `Entity/Model/Column/PrimaryKey` em `core` (metadados estáticos;
      `PrimaryKey` é blanket: `Into<Value> + Clone + Send + Sync + 'static`) +
      `OrmError::Database`.
- [x] `ColumnRef<T>` + `TextColumn` em `query` (`Expr::compare` virou `pub`;
      `eq/ne(Null)` → `is_null/is_not_null`).
- [x] `FromRow` em `driver` + `DriverError::Server { code, message }` (usa
      `tiberius::Error::code()`) nos pontos de conversão.
- [x] `#[derive(Entity)]`/`#[derive(Model)]` em `derive` (`#[orm(table, schema)]`,
      `#[orm(primary_key, auto, column)]`): gera `{Struct}Columns` com
      `ColumnRef<T>`, `FromRow` lê **por nome**, `to_values` omite PK auto,
      `set_id` via `FromValue`.
- [x] `DbContext`/`DbSet`/`Select` em `ef-orm`: `insert` (identity via
      `OUTPUT INSERTED` → `set_id`), `find(pk)` → `NotFound`, `update`/`delete`
      → `bool`, `filter(|c| …)` tipado, `order_by` por coluna, `page`/`first`;
      facade converte `DriverError` → `OrmError` (2601/2627/547/515 →
      `Constraint`, 241/245/8114/8115 → `Conversion`, resto → `Database`).
- [x] Mapeamento uuid/chrono/decimal atrás das features `with-uuid`/`with-chrono`/
      `with-rust_decimal` (core/driver/driver-tds/facade; habilita
      `tiberius/chrono` e `tiberius/rust_decimal`; o uuid do tiberius é
      incondicional, então `with-uuid` só encaminha as features internas).
- **Aceite**: `EF_ORM_TEST_DB_PASSWORD=… cargo run -p ef-orm-examples --example
  hello-crud` contra SQL Server real — cria tabela idempotente, insere (identity
  escrita de volta), lê, atualiza, seleciona com filtros, remove e prova
  `NotFound`, com round-trip de uuid/chrono/decimal no caminho ✔
- **Suíte**: `fmt --check` + `clippy --all-targets -D warnings` + **86 testes
  verdes** (core 11, driver 12, driver-tds 14, query 29, facade 20).
- Desvios/decisões anotados:
  - Colunas geradas como **struct `{Struct}Columns` de `ColumnRef<T>`** em vez
    de enum de colunas (SeaORM-style): o predicado é tipado pelo tipo Rust da
    coluna; `like` só compila em `TextColumn` (`String`/`Option<String>`).
  - `DriverError::Query` (fase 1) foi **renomeado** para
    `Server { code: Option<u32>, message: String }` — o código TDS é o que
    permite ao facade distinguir constraint de erro comum.
  - `FromRow` mora no `driver` (o `core` não pode depender de `Row`); a facade
    pede o bound combinado `T: Model + FromRow`.
  - PK única apenas (composta → erro de derive); `auto` exige `primary_key`;
    campo anulável precisa ser `Option<T>` literal (alias não reconhecido);
    a facade usa `extern crate self as ef_orm;` p/ caminhos auto-referentes.
  - `INSERT` com PK auto usa `query()` (não `execute`) + `OUTPUT INSERTED.[pk]`;
    `SELECT TOP` renderiza `TOP (@P1)` com parâmetro bindado (snapshot corrigido).
  - Sem change tracking: `update`/`delete` devolvem `bool` (fase 4 vira
    `Conflict`); `Value::Uuid/DateTime/Decimal` só existem com a feature ligada.
  - `datetimeoffset` é lido como wall-clock (`naive_local`, offset descartado);
    `date` vira `NaiveDateTime` à meia-noite; `time`/`xml`/`sql_variant` seguem
    sem variante `Value` (erro de protocolo).
  - Deps novas com justificativa no workspace: `syn`/`quote`/`proc-macro2`
    (derives), `uuid`/`chrono`/`rust_decimal` (features `with-*`); dev-deps na
    facade só p/ testes dos derives; exemplos habilitam as três features.

## Fase 4 — Change tracking (diferencial EF Core) — **concluída**
- [x] `ChangeTracker` com snapshot por entidade (Inserted/Modified/Deleted/Unchanged).
- [x] `save_changes()` como unit of work: diff → statements numa transação.
- [x] Concurrencia otimista: `WHERE` com valores originais → `OrmError::Conflict`.
- [x] `as_no_tracking()` para leituras.
- **Aceite**: exemplo `change-tracking.rs` demonstra diff, rollback e conflito ✔
  (rodado contra SQL Server real: `inserted=2`, conflito em `b` → rollback
  provado com `a='alpha v2'`, retry `updated=2` e `deleted=2`; `hello-crud`
  também reproduzido com a API staged).
- Implementação:
  - `Tx` ganhou `execute`/`query` (o `save_changes` roda os statements dentro
    da transação; `TdsTx` delega aos helpers `run_execute`/`run_query`).
  - Escritas ficam **staged** e são síncronas: `insert(model) -> StagedInsert<T>`
    (model por valor), `update(&T)`/`delete(id)` montam o statement na hora;
    `save_changes()` faz `take_staged` → checkout → `begin` → flush → `commit`
    → write-backs → `finish`, devolvendo `SaveChangesReport`.
  - Leitura (`find`/`select`) faz snapshot dos originais; `Select::as_no_tracking()`
    desliga o tracking.
  - `staged.rs`: `StagedInsert` guarda `Arc<Mutex<Shared<T>>>`; write-back
    pós-commit escreve a chave de `OUTPUT INSERTED` e `resolve()` só entrega o
    modelo depois que o handle é único e o save commitou.
  - `concurrency.rs`: `update_tracked` (diff `SET` + `WHERE pk AND <originais>`,
    `is_null` p/ originais `NULL`) e `delete_guarded`.
- Desvios/decisões anotados:
  - `insert` é **síncrono e por valor** (devolve o ticket); nada é escrito antes
    de `save_changes`. `update`/`delete` também são síncronos (`Result<(), OrmError>`).
  - `affected == 0` em `UPDATE`/`DELETE` agora é `OrmError::Conflict` (na fase 3
    `update`/`delete` devolviam `bool`); a transação sofre rollback e as mudanças
    voltam à frente da fila para retry.
  - Guarda de concorrência usa **todas as colunas não-chave originais** (não só
    as alteradas); snapshot de linha lida sem tracking degrada para `WHERE pk`.
  - `delete` de uma linha cujo `INSERT` ainda está staged apenas cancela o
    insert (no-op, como no EF).
  - `Entity` passou a exigir `'static` e `Tx: Send` (futures `Send` do unit of work).
  - `OrmError::Validation` novo (ex.: `resolve()` antes de um save bem-sucedido).
  - `ChangeTracker` vive em `DbContext` atrás de `Mutex`; guardas só em trechos
    síncronos (nunca atravessam `.await`).

## Fase 5 — Migrations — **concluída**
- [x] `Schema` builder (tabela/coluna/índice/FK) sobre a AST.
- [x] Engine: diff de modelo → DDL T-SQL; tabela `__ef_migrations`.
- [x] CLI: `migrate up|down|status`, `scaffold` (gerar Entity a partir do banco via `INFORMATION_SCHEMA`).
- **Aceite**: CLI sobe/desce migration num banco limpo e reporta status ✔
  (rodado contra SQL Server real: `migrate up` → `applied 900`, `status` →
  `applied 900 cli_demo`, `migrate down` → `rolled back 900`; `scaffold --out`
  gerou os entities; o exemplo code-first `migrations.rs` aplicou `001`/`002`,
  fez introspecção, diff, scaffold e rollback completo).
- Implementação:
  - `ef-orm-query`: AST de DDL (`ddl.rs` + `mssql/ddl.rs`) com `CreateTable`,
    `DropTable`, `AddColumn`, `DropColumn`, `AlterColumn`, `CreateIndex`,
    `DropIndex`, `AddForeignKey`, `DropConstraint`; `SqlType` (22 variantes,
    incl. `NVARCHAR(MAX)`/`DECIMAL(p,s)`), `ColumnDef` builder e
    `DefaultValue::{Literal, SysUtcDateTime}`. Todos viram `Statement` e passam
    pelo mesmo `Dialect::build` (placeholders/quoting compartilhados).
  - `ef-orm-migrate`: `Schema` (acumulador de statements), `Migration` (`up`/`down`
    como fn pointers), `Migrator` (`ensure_history`, `applied`, `up`, `down`,
    `status`) e `SqlMigration` (modo arquivos SQL). `__ef_migrations`
    (`version` PK, `name`, `applied_at DEFAULT SYSUTCDATETIME()`) é criada
    sob demanda; **cada migração + linha de histórico roda numa transação**.
  - Introspecção (`INFORMATION_SCHEMA` + `sys`): sem JOINs (a AST ainda não
    tem), os conjuntos pequenos são cruzados em Rust; detecta PK via
    `TABLE_CONSTRAINTS`/`KEY_COLUMN_USAGE` e identity via
    `sys.identity_columns` + `sys.tables`.
  - `diff::diff(expected, actual)` (tabelas/colunas: create/add/alter/drop) e
    `scaffold::scaffold(live)` → fonte Rust com `#[derive(Entity, Model)]`.
  - `ef-orm-cli` (clap): flags/`EF_ORM_DB_*`, `migrate up|down|status --dir` e
    `scaffold [--out]`.
- Desvios/decisões anotados:
  - Migrações **code-first** usam a AST (`Migration`); a CLI aplica pares
    `.up.sql`/`.down.sql` (`<version>_<name>`) — o SQL é do usuário, mas ainda
    roda em transação e é registrado no histórico. BOM UTF-8 é tolerado.
  - O `diff` é **convergente**: derruba tabelas/colunas que existem no banco e
    não no alvo (documentado como operação de revisão obrigatória).
  - Defaults em DDL são **inline** (não bindados): literais escapados
    (`N'…'`, `0x…`) e `SYSUTCDATETIME()`.
  - `ALTER COLUMN` cobre tipo/nulabilidade apenas (IDENTITY/DEFAULT exigem
    constraint nomeada).
  - `ef-orm-cli` depende de `ef-orm-driver-tds`, `tokio` e `clap` (binário folha;
    nenhuma biblioteca de cima importa a CLI).
  - `ef-orm` reexporta `Tx` (o engine de migrations o consome).
  - Dep nova: `clap` (fase 5, justificada no workspace).
- **Suíte**: `fmt --check` + `clippy --all-targets -D warnings` + **134 testes
  verdes** (query 44, migrate 13, cli 1, facade 39, core 11, driver 12, tds 14).

## Fase 6 — Polimento e lançamento — **concluída (exceto publicação no crates.io)**
- [x] Streaming de resultados (`Stream` de linhas) e `try_get` parcial de colunas.
- [x] Eager loading (`with_related`) para evitar N+1.
- [x] Bulk copy (Tiberius `BulkLoadRequest`) para inserts em massa.
- [x] Raw SQL escape hatch (`ctx.raw_sql(...)`).
- [ ] Docs, site/repo, publicação `0.1.0` no crates.io.
- Implementação:
  - `raw_sql.rs`: `DbContext::raw_sql(sql)` → `param(..)` + `execute`/`rows`/
    `fetch_all::<T>`/`fetch_one::<T>` (colunas lidas por nome, projeção parcial ok).
  - `Select::columns(..)` troca a projeção e `all_as`/`first_as` mapeiam para um
    `FromRow` parcial (sem tracking — a linha é incompleta).
  - Streaming: `Conn::query_stream(sql: String, params: Vec<Value>)` (default
    vazio) e `TdsConn` via `async_stream::try_stream!` sobre o
    `QueryStream::into_row_stream()` do Tiberius (o gerador **possui** SQL/params e
    empresta só o cliente). A facade expõe `Select::stream(&mut conn)` sobre
    `DbContext::checkout()`, devolvendo `Pin<Box<dyn Stream<Item = Result<T, OrmError>>>>`.
  - Bulk: `Conn::bulk_insert(table, columns, rows)` (default "unsupported") e
    `TdsConn` via `BulkLoadRequest` (`column_data` → `TokenRow`); facade
    `DbSet::insert_many(&[T])`.
  - Anti-N+1: `DbSet::find_in(column, keys)` carrega todos os filhos em **uma**
    query (`IN`); o agrupamento por pai fica em Rust (HashMap).
  - Exemplo `advanced.rs` cobre raw SQL, streaming, bulk, find_in e projeção
    parcial contra SQL Server real.
- **Aceite** (E2E real, `advanced`): `bulk authors: 2`, `bulk posts: 3`,
  `raw count for 'ada': 2`, `streamed posts: 3`, `Ada → 2 posts / Grace → 1`,
  `partial projection: 3` ✔
- Desvios/decisões anotados:
  - Eager loading **sem metadados de relação** (o derive não modela FKs): é um
    `find_in` em lote + agrupamento explícito, não um `with_related` mágico.
  - Streaming exige uma conexão emprestada (`ctx.checkout()`), não faz tracking
    (linhas consumidas uma a uma) e custa um `Box::pin` por query.
  - `insert_many` não lê `OUTPUT`: chaves `IDENTITY` **não** voltam ao modelo
    (use `insert` + `save_changes` quando precisar delas).
  - Deps novas justificadas: `futures-core` (trait `Stream` no driver),
    `futures-util` (combinators; já vinha via Tiberius) e `async-stream`
    (gerador que evita struct auto-referencial no streaming).
  - Publicação `0.1.0` no crates.io **não executada** (exige token/credenciais);
    README, rustdoc e metadados de pacote estão prontos.
- **Suíte**: `fmt --check` + `clippy --all-targets -D warnings` + **139 testes
  verdes** (facade 44, query 44, migrate 13, core 11, driver 12, tds 14, cli 1).

## Riscos abertos
- MERGE + concorrência (estudar `HOLDLOCK` antes da fase 4/6).
- ~~Abstração do pool sem tokio nos tipos públicos (validar RPITIT na fase 1)~~
  — validado: tokio só em campos privados (semáforo/timeout) e traits RPITIT
  desaguardados com `+ Send`; nenhuma superfície pública expõe tokio.
- Nomes no crates.io: checar disponibilidade de `ef-orm` antes do lançamento.
