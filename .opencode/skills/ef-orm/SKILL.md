---
name: ef-orm
description: Desenvolve o ef-ORM, um ORM Rust MSSQL-first construído do zero (Tiberius + T-SQL, arquitetura limpa). Use ao trabalhar nos crates ef-orm, no dialeto SQL Server, em migrations, change tracking ou em qualquer arquivo deste repositório.
---

# Skill: ef-ORM

ORM Rust para SQL Server, inspirado no **SeaORM** (entity traits, active models, fluent query)
e no **EF Core** (DbContext, change tracking, unit of work, migrations), construído do zero com
arquitetura limpa. Nome mantido `ef-orm`. Licença alvo: MIT OR Apache-2.0.

## Decisões firmadas (não reabrir sem pedido do usuário)

- **Estratégia: do zero, MSSQL-first.** Sem fork de SeaORM/SeaQuery, sem dependência do sqlx.
- **Driver: Tiberius** (protocolo TDS). É o único crate que fala TDS.
- **Dialeto T-SQL próprio** (AST própria em `ef-orm-query`).
- **Runtime: tokio** como default (Tiberius é runtime-independente; abstrações não podem expor tipos tokio no `core`).

## Leitura obrigatória antes de codar

1. `references/architecture.md` — workspace, crates, regra de dependência e fluxo de uma query.
2. `references/conventions.md` — tamanho de arquivos, estilo, erros, testes, política de deps.
3. `references/mssql-dialect.md` — regras de tradução Rust → T-SQL e mapa de tipos.
4. `references/roadmap.md` — fase atual, tarefas e critérios de aceite. **Atualize o roadmap ao terminar uma fase.**

## Workflow padrão

1. Identifique a **fase** no roadmap e o **crate** alvo pela regra de dependência.
2. Escreva arquivos pequenos (máx. ~300 linhas); um conceito principal por arquivo.
3. `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` antes de concluir.
4. Testes de dialeto são **snapshot de string SQL** (`assert_eq!` do SQL gerado) — sempre que alterar
   o dialeto, adicionar/atualizar o teste correspondente.
5. Ao terminar: marcar tarefa no roadmap e resumir o que mudou ao usuário.

## Regra de dependência (arquitetura limpa — nunca violar)

Seta `→` = **depende de**:

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

- Nada importa "para cima". `core` não conhece SQL, drivers nem tokio.
- Só `ef-orm-driver-tds` pode importar `tiberius`.
- Só `ef-orm-query` pode gerar strings de SQL.

## Glossário rápido

| Conceito ef-orm | Inspiração |
|---|---|
| `Entity`/`Model`/`Column` traits + derive | SeaORM |
| `DbContext`, `DbSet`, `ChangeTracker`, `SaveChanges` | EF Core |
| `Driver`/`Conn`/`Tx` traits | Substitui o acoplamento sqlx do SeaORM |
| `Dialect` + `MssqlDialect` | Substitui os query builders do SeaQuery |
