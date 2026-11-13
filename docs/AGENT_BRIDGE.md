# 本地助手桥（Local agent bridge）

ai.md 可以给**这台电脑上**的其它工具（例如本地的 AI 助手）开一个小接口，让它们读取你的文档和目录、新建文档。它默认**关闭**；在 设置 › 通用 打开「本地助手桥」后才工作。

- 只监听 `127.0.0.1`（本机地址），从不联网，也没有遥测。
- 打开后，文档库根目录会出现 `agent-bridge.json`，里面是端口和口令：

```json
{ "app": "ai.md", "port": 51234, "token": "…", "pid": 1234 }
```

- 关掉开关即停止监听并删除这个文件。口令在第一次打开时生成，之后不变（除非你手动删改设置）。
- 每个请求都在应用的主线程上对着真实文档库执行，不会和界面抢数据。

## 协议

连接 `127.0.0.1:<port>`，发送**一行** JSON，读回**一行** JSON，连接关闭。

请求：

```json
{ "id": 1, "token": "…", "method": "list_docs", "params": {} }
```

响应：

```json
{ "id": 1, "ok": true, "result": … }
{ "id": 1, "ok": false, "error": "…" }
```

## 方法

| 方法 | 参数 | 结果 |
|---|---|---|
| `ping` | — | 应用名、版本、方法列表 |
| `list_docs` | — | 全部文档的 `id`、`title`、`modified` |
| `read_doc` | `id` | 文档的 `id`、`title` 和 Markdown 全文 |
| `search_docs` | `query` | 标题或正文命中的文档（最多 20 篇） |
| `search_catalog` | `query` | 目录命中的条目（最多 20 条） |
| `create_doc` | `title`、`body`（纯文本，一行一段） | 新文档的 `id` |

桥只能读和新建，不能改、不能删。删除和编辑请在 ai.md 里进行。

## 通过 MCP 使用（Using it through MCP）

ai.md 附带了一个小的 MCP 适配器 `ai_md_mcp.exe`，和 `ai_md.exe` 放在一起（安装版在安装文件夹里，便携版在解压后的文件夹里）。它把标准的 Model Context Protocol 工具调用转发给上面的桥，所以支持 MCP 的客户端（Claude、Cursor、VS Code 等）不用懂桥的私有协议就能接入。

用法：

1. 在 ai.md 里打开 设置 › 通用 › 本地助手桥，并保持 ai.md 运行。
2. 把适配器加进你的 MCP 客户端配置，例如：

```json
{
  "mcpServers": {
    "ai.md": {
      "command": "C:\\Program Files\\ai.md\\ai_md_mcp.exe"
    }
  }
}
```

（便携版把路径换成你解压的位置。）

适配器提供五个工具：`list_documents`、`read_document`、`search_documents`、`search_catalog`、`create_document`，和桥的方法一一对应。桥没打开时，工具会直接说明去哪里打开——适配器自己不保存任何状态，每次调用都会重新读取 `agent-bridge.json`，所以重启 ai.md 不会让它失效。
