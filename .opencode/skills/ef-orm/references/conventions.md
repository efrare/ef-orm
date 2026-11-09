# Convenções do ef-ORM

## Tamanho e estrutura de arquivos

- **Máximo ~300 linhas por arquivo.** Passou? Divida em módulos (`mod x;` + `x/`).
- Um conceito principal por arquivo (ex.: `change_tracker.rs`, `unit_of_work.rs`, `pool.rs`).
- Ordem dentro de um arquivo: imports → tipos públicos → impls → testes unitários (`#[cfg(test)]`).
- Módulos privados por default; só exporta em `lib.rs` o que é API pública.

## Estilo Rust

- `#![forbid(unsafe_code)]` em todos os crates.
- `cargo fmt` + `clippy -D warnings` obrigatórios; sem `unwrap()` em código de biblioteca
  (só em testes); erros com `thiserror` na biblioteca, `anyhow` só em CLI/exemplos.
- Nomes: `snake_case` itens, `PascalCase` tipos, traits de domínio curtos (`Entity`, `Column`).
- Async: `async fn` + `impl Future` nos traits (RPITIT), sem `async-trait` salvo necessidade
  de object-safety (se precisar, isolar no crate de driver).
- Dependências novas exigem justificativa no PR/comentário; preferir std + poucas deps maduras.

## Estratos de erro

- `ef-orm-core::OrmError` — erros de domínio (NotFound, Conversion, Conflict, Constraint).
- `ef-orm-driver::DriverError` — infra (Connect, Timeout, Protocol, Closed).
- A facade converte `DriverError → OrmError` na fronteira; o usuário final só vê `OrmError`.

## Testes

- **Dialeto**: snapshot string (`assert_eq!(sql, "…")`) por sentença; cobrir SELECT/INSERT/UPDATE/
  DELETE/MERGE/paginação/OUTPUT/escaping.
- **Unitário**: lógica pura (AST, change tracker, mapa de tipos) sem I/O.
- **Integração** (feature `itest`): SQL Server real via Docker/testcontainers; rodar só quando
  `EF_ORM_TEST_DB` estiver setada — CI sem banco não quebra.
- Cada fix de bug vira teste de regressão.

## Documentação

- Rustdoc em toda API pública com exemplo compilável quando possível.
- `examples/` executáveis: `hello-crud`, `change-tracking`, `migrations`.
- Decisões de design ficam nos `references/` desta skill (atualizar junto com o código).

## Commits / progresso

- Escopo pequeno por commit; mensagem no imperativo ("add MssqlDialect pagination").
- Ao concluir tarefa do roadmap: marcar `[x]` e anotar desvios/arquitetura nova em `roadmap.md`.
