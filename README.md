# 📊 New Report - Data Engine

Motor SQL moderno para bases DBISAM, com leitura binária via mmap e execução SQLite.

O **New Report** é um utilitário moderno desenvolvido em **Rust** para substituição de ferramentas legadas e lentas de relatórios. Ele permite a execução de **SQL (SQLite)** sobre bases de dados **DBISAM `.dat`**, garantindo performance extrema através de processamento nativo e mapeamento de memória.
> ⚡ Relatórios que antes levavam minutos agora executam em segundos.

O projeto foca na geração e visualização de relatórios de alta performance, eliminando as limitações do motor DBISAM original.

---
## 🎯 Problema que Ele Resolve

Motores DBISAM tradicionais:

* ❌ Não suportam SQL moderno (JOIN complexo, subqueries)
* ❌ São lentos para grandes volumes
* ❌ Travavam com consultas pesadas
* ❌ Limitavam análise de dados

O **New Report** resolve isso criando uma camada moderna de execução SQL sobre os dados binários originais.

---

## ⚠️ Dependência Obrigatória: `schema.toml`
O funcionamento deste motor depende do arquivo `schema.toml`, que contém os offsets e tipos de dados das tabelas binárias dos arquivos `.dat`.
Este arquivo deve ser gerado pelo utilitário DBISAM-Scan, que faz parte do projeto:
👉 [DBISAM-Translate](https://github.com/murilo813/DBISAM-Translate)
Após gerar o `schema.toml`, copie para a raiz do projeto.

---
## 🧱 Arquitetura Interna
* 🦀 **Rust:** Performance nativa
* 🗂️ **Memory Mapping (mmap):** Leitura ultra rápida dos `.dat`
* 🗃️ **SQLite (WAL Mode):** Execução SQL robusta e concorrente
* 🖥️ **Dioxus:** Interface moderna e reativa
* 🔒 **Read-Only Engine:** Os arquivos `.dat` nunca são modificados

---

## 📁 Estrutura do Motor

### 🔄 **Sincronização Dinâmica** 
Antes da execução da query, o motor processa a tag:
```SQL
[SYNC: ...]
```
Ela define:
* Quais tabelas serão carregadas
* Quais colunas serão extraídas

Isso reduz drasticamente o uso de memória e aumenta a performance.
> Caso queira puxar todas as colunas de uma tabela use `[SYNC: tabela(*)]`

---

### 🚀 Como usar
#### Configuração
**Crie um arquivo `.env` na raiz do projeto para apontar para suas bases:**
```env
DATABASE_PATH=C:\Caminho\Para\Bases\Dat
```
#### Baixar executável
**Acesse:**
👉 [Releases](https://github.com/murilo813/New-Report/releases)
Baixe o `NewReport.exe`.

#### Executando uma Query
```SQL
[SYNC: tabela1(id, numero, CFOP), tabela2(id_nf, custo_liq)]

SELECT 
    b.numero, 
    a.custo_liq 
FROM tabela2 b
INNER JOIN tabela1 a ON a.id_nf = b.id
ORDER BY b.data_entrada DESC
LIMIT 100;
```

## Licença

Este projeto está licenciado sob a Licença MIT - veja o arquivo [LICENSE](./LICENSE) para detalhes.

Desenvolvido com ❤️ por Murilo de Souza