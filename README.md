# 📊 New Report - Data Engine

Motor SQL moderno para bases DBISAM, com leitura binária via **mmap**, processamento colunar **Apache Arrow** e execução vetorizada **DataFusion**.

O **New Report** é um utilitário de últime geração desenvolvido em **Rust** para substituição de ferramentas legadas e lentas de relatórios. Ele permite a execução de **SQL Moderno** sobre bases de dados **DBISAM `.dat`**, garantindo performance extrema através de processamento nativo e mapeamento de memória.
> ⚡ Relatórios que levavam 5 minutos no motor original agora executam em menos de 5 segundos.

O projeto foca na geração e visualização de relatórios de alta performance, eliminando as limitações do motor DBISAM original.

---
## 🎯 Problema que Ele Resolve

Motores DBISAM tradicionais:

* ❌ Não suportam SQL moderno (Window Functions, Joins complexos, CTEs).
* ❌ São lentos para grandes volumes
* ❌ Travavam com consultas pesadas
* ❌ Limitavam análise de dados

O **New Report** resolve isso criando uma camada de **Execução Vetorizada** diretamente sobre os dados binários.

---

## 🔧 Escovação de Bits: Por que é tão rápido?
A performance extrema do New Report não é por acaso; é fruto de engenharia de baixo nível:

* **Zero-Copy Memory Mapping (mmap)**: Os arquivos `.dat` são mapeados diretamente no espaço de endereçamento da memória. O sistema operacional gerencia o cache de disco, eliminando a necessidade de ler o arquivo repetidamente para o buffer da aplicação.
* **Arquitetura Colunar (Apache Arrow)**: Diferente de bancos tradicionais que leem "linhas", o **New Report** organiza os dados em colunas na RAM. Isso permite que a CPU processe milhares de registros de uma vez só usando instruções **SIMD**.
* **Execução Vetorizada (DataFusion)**: Utilizamos o motor de consulta do Apache **DataFusion**. As queries são compiladas e executadas de forma paralela entre os núcleos da CPU, garantindo que JOINS e agregações (SUM, COUNT) ocorram na velocidade máxima da memória.
* **Morte ao "Insert"**: Ao contrário de soluções que importam dados para o SQLite, o **New Report** registra os dados no formato Arrow instantaneamente (0ms de overhead de registro).

---
## ⚠️ Dependência Obrigatória: `schema.toml`
O funcionamento deste motor depende do arquivo `schema.toml`, que contém os offsets e tipos de dados das tabelas binárias dos arquivos `.dat`.
Este arquivo deve ser gerado pelo utilitário DBISAM-Scan, que faz parte do projeto:
👉 [DBISAM-Translate](https://github.com/murilo813/DBISAM-Translate)
Após gerar o `schema.toml`, copie para a raiz do projeto.

---
## 🧱 Arquitetura Interna
* 🦀 **Rust:** Performance e Segurança de Memória
* 🗂️ **Memory Mapping (mmap):** Leitura ultra rápida dos `.dat`
* 🗃️ **Apache Arrow:** Memória Colunar
* 🗃️ **DataFusion:** SQL Vetorizado
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

Isso reduz o uso de memória, aumenta a performance e diminui margem de erros.
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
ORDER BY b.data_entrada DESC;
```

## Licença

Este projeto está licenciado sob Licença - veja o arquivo [LICENSE](./LICENSE) para detalhes.

Desenvolvido com ❤️ por Murilo de Souza
