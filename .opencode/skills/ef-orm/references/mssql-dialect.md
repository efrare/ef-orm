# Dialeto T-SQL (MssqlDialect) — regras de tradução

Fonte de verdade para todo SQL gerado. Qualquer divergência vira teste snapshot.

## Sintaxe diferencial vs. Postgres/MySQL

| Conceito | T-SQL (correto) | Equivoco comum (ERRADO) |
|---|---|---|
| Paginação | `ORDER BY … OFFSET @p ROWS FETCH NEXT @p ROWS ONLY` (`ORDER BY` é **obrigatório**) | `LIMIT/OFFSET` |
| Top-N sem página | `SELECT TOP (@p) …` | `LIMIT` |
| Retornar IDs | `OUTPUT INSERTED.id` no INSERT/UPDATE/DELETE | `RETURNING` |
| Upsert | `MERGE INTO t USING … WHEN MATCHED THEN … WHEN NOT MATCHED THEN …;` (+ `;` antes de MERGE) | `ON CONFLICT` / `ON DUPLICATE KEY` |
| Identidade | `id INT IDENTITY(1,1) NOT NULL PRIMARY KEY` | `AUTO_INCREMENT` / `SERIAL` |
| Identidade inserida | `OUTPUT INSERTED.id` (evita `SCOPE_IDENTITY()` extra) | `last_insert_id()` |
| Concat | `a + b` | `a \|\| b` |
| Booleando | `WHERE active = @p` com `BIT` (aceita 0/1; comparar com `= 1` é mais seguro) | `TRUE/FALSE` |
| Isnull | `ISNULL(a, b)` (ou `COALESCE`, este funciona) | `IFNULL` |
| Quoting | `[identificador]` (aspas duplas só com QUOTED_IDENTIFIER ON) | `` ` `` ou `"` default |
| Nomes qualificados | `[schema].[tabela].[coluna]`, schema default `dbo` | schema implícito só |
| Strings | `NVARCHAR` para Unicode (prefixo `N'…'` em literais não-ASCII) | `VARCHAR` silencioso |
| Auto-commit de DDL | Sempre explícito; migrations rodam em transação quando possível | assumir autocommit |
| Comentário | `--` e `/* */` | `#` (MySQL) |

## Regras do builder

1. **Placeholder `@P1, @P2…`** na ordem de avaliação da AST; valores viabindados nunca interpolados.
2. `OFFSET/FETCH` só com `ORDER BY` — se o usuário não ordenar, injetar `ORDER BY (SELECT NULL)`
   apenas quando houver paginação; caso contrário não gerar ORDER BY.
3. `MERGE` exige `;` imediatamente antes e `MATCHED`/`NOT MATCHED` com condições completas.
4. `UPDATE`/`DELETE` com `OUTPUT` funcionam; em `MERGE`, `OUTPUT` vai no final do bloco alvo.
5. String de coluna: sempre tipada (`NVARCHAR(n)`); `n` do metadata (255 default, `MAX` para texto longo).
6. Nada de atalhos: DDL por statement (`ALTER TABLE … ADD` separado; não existe `ADD COLUMN` de uma vez
   para múltiplas colunas — uma por statement, regra do T-SQL).
7. `TOP` com expressão precisa de parênteses: `TOP (@P1)`.

## Mapa de tipos Rust ⇄ TDS

| Rust (Value) | TDS/T-SQL (escrita) | Leitura (`Row::get`) |
|---|---|---|
| `bool` | `BIT` | `BIT → bool` |
| `i8/i16/i32` | `SMALLINT/INT` | `INT → i32` |
| `i64` | `BIGINT` | `BIGINT → i64` |
| `f32/f64` | `REAL/FLOAT` | `FLOAT → f64` |
| `String`/`&str` | `NVARCHAR(n)` | `NVARCHAR → String` |
| `Vec<u8>` | `VARBINARY(n)` | `VARBINARY → Vec<u8>` |
| `Uuid` | `UNIQUEIDENTIFIER` | `UNIQUEIDENTIFIER → Uuid` (feature `with-uuid`) |
| `DateTime<Utc>` | `DATETIME2(7)` | `DATETIME2 → DateTime<Utc>` (feature `with-chrono`) |
| `DateTime<Local>` | `DATETIMEOFFSET(7)` | → `DateTime<FixedOffset>` |
| `Decimal` | `DECIMAL(p,s)` (feature `with-rust_decimal`) | `DECIMAL → Decimal` |
| `Option<T>` | coluna `NULL`/`NOT NULL` | `NULL → None` |
| `serde_json::Value` | `NVARCHAR(MAX)` + `FOR JSON`/`JSON_VALUE` (fase futura) | — |

Cuidados:
- Escala `DECIMAL(p,s)` vem dos metadados da coluna; nunca truncar silenciosamente (erro `Conversion`).
- `DATETIME2` perde fuso: normalizar para UTC na borda; `DATETIMEOFFSET` preserva.
- `BIT` não aceita `NULL` implícito: usar `Option<bool>`.
- Decodificação por **nome** preferencialmente (robusto a `SELECT *`), por índice só em projeções fixas.

## Concorrência e transações

- Isolation level default: `READ COMMITTED`; expor `SNAPSHOT` (SQL Server tem `ALLOW_SNAPSHOT_ISOLATION`
  e `READ_COMMITTED_SNAPSHOT`) como opção do `DbContext`.
- `UPDATE … WHERE pk = @valor_original` → 0 linhas afetadas = `OrmError::Conflict`.
- `MERGE` deve ser sempre acompanhado de `HOLDLOCK`/`UPDLOCK` quando houver race (revisar por fase).
