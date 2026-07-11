# 创建 Hermes 风格插件 🦀

本指南演示如何创建一个与 EdgeCrab 的 Hermes 兼容层一起工作的 Python 插件。

## 你需要什么

- Python 3.10+
- EdgeCrab 二进制文件

## 插件布局

最小的 Hermes 插件如下所示：

```text
my-plugin/
├── plugin.yaml
├── __init__.py
└── SKILL.md
```

### `plugin.yaml`

这是清单文件：

```yaml
name: my-plugin
description: 一个示例插件
version: "0.1.0"
author: Your Name
compatibility:
  hermes: ">=0.1.0"
  edgecrab: ">=0.1.0"
requires_env: []
```

### `__init__.py`

这是主要入口点。它必须导出一个 `register` 函数：

```python
from hermes.constants import HERMES_TOOL_ERROR

def register(agent):
    agent.tools.register_tool(
        name="hello_world",
        description="打印一条问候消息",
        arguments={},
        tool_error=HERMES_TOOL_ERROR,
        handler=lambda args: {"result": "Hello, World!"}
    )
```

### `SKILL.md`（可选）

这是技能文档。如果存在，它会被注入到系统提示中：

```markdown
# Hello World 技能

使用 `hello_world` 工具向世界问好。
```

## 工具注册

### 基础工具

```python
def register(agent):
    def greet(args):
        name = args.get("name", "World")
        return {"result": f"Hello, {name}!"}
    
    agent.tools.register_tool(
        name="greet",
        description="向某人问好",
        arguments={
            "name": {"type": "string", "description": "要问候的人的名字"}
        },
        tool_error=HERMES_TOOL_ERROR,
        handler=greet
    )
```

### 使用 Pydantic 模式

```python
from pydantic import BaseModel, Field

class GreetArgs(BaseModel):
    name: str = Field(description="要问候的人的名字")

def register(agent):
    def greet(args):
        args = GreetArgs(**args)
        return {"result": f"Hello, {args.name}!"}
    
    agent.tools.register_tool(
        name="greet",
        description="向某人问好",
        arguments=GreetArgs.model_json_schema(),
        tool_error=HERMES_TOOL_ERROR,
        handler=greet
    )
```

## 钩子

### `on_session_start`

```python
def register(agent):
    def on_start(session_id):
        print(f"会话 {session_id} 已启动")
    
    agent.hooks.on_session_start(on_start)
```

### `pre_tool_call` 和 `post_tool_call`

```python
def register(agent):
    def pre_call(tool_name, args):
        print(f"即将调用 {tool_name} 并使用 {args}")
    
    def post_call(tool_name, args, result):
        print(f"{tool_name} 返回了 {result}")
    
    agent.hooks.pre_tool_call(pre_call)
    agent.hooks.post_tool_call(post_call)
```

### `pre_llm_call`

```python
def register(agent):
    def pre_llm(messages):
        print(f"即将向 LLM 发送 {len(messages)} 条消息")
        return messages
    
    agent.hooks.pre_llm_call(pre_llm)
```

### `post_llm_call`

```python
def register(agent):
    def post_llm(response):
        print(f"LLM 返回了 {len(response.content)} 个字符")
        return response
    
    agent.hooks.post_llm_call(post_llm)
```

### `on_session_end`

```python
def register(agent):
    def on_end(session_id):
        print(f"会话 {session_id} 已结束")
    
    agent.hooks.on_session_end(on_end)
```

## 使用内存

```python
def register(agent):
    def store_data(args):
        key = args["key"]
        value = args["value"]
        agent.memory_provider.store(key, value)
        return {"result": f"已存储 {key}"}
    
    def get_data(args):
        key = args["key"]
        value = agent.memory_provider.get(key)
        return {"result": value}
    
    agent.tools.register_tool(
        name="store_data",
        description="在内存中存储数据",
        arguments={
            "key": {"type": "string", "description": "数据键"},
            "value": {"type": "string", "description": "数据值"}
        },
        tool_error=HERMES_TOOL_ERROR,
        handler=store_data
    )
    
    agent.tools.register_tool(
        name="get_data",
        description="从内存中获取数据",
        arguments={
            "key": {"type": "string", "description": "数据键"}
        },
        tool_error=HERMES_TOOL_ERROR,
        handler=get_data
    )
```

## 需要环境变量

```yaml
name: my-plugin
description: 需要 API 密钥的插件
version: "0.1.0"
requires_env:
  - MY_API_KEY
```

如果 `MY_API_KEY` 未设置，插件将被标记为 `setup-needed`，其工具不会暴露。

## CLI 子命令

如果插件包含 `cli.py` 文件，它可以注册 CLI 子命令：

```python
def register_cli(subparser):
    parser = subparser.add_parser("my-plugin", help="我的插件命令")
    parser.add_argument("--action", choices=["list", "show"], default="list")
    parser.set_defaults(func=run)

def run(args):
    if args.action == "list":
        print("列出项目...")
    elif args.action == "show":
        print("显示项目...")
```

然后你可以运行：

```bash
edgecrab my-plugin --action list
```

## 安装

### 本地安装

```bash
edgecrab plugins install ./my-plugin
```

### 从 GitHub 安装

将你的插件推送到 GitHub，然后：

```bash
edgecrab plugins install github:your-username/your-repo/path/to/plugin
```

## 调试

### 启用调试日志

```bash
RUST_LOG=trace edgecrab plugins list
```

### 检查插件状态

```bash
edgecrab plugins info my-plugin
```

### 测试工具

```bash
edgecrab tools call hello_world
```

## 完整示例

这里是一个完整的计算器插件：

```text
calculator/
├── plugin.yaml
├── __init__.py
├── schemas.py
├── tools.py
└── SKILL.md
```

**`plugin.yaml`**:

```yaml
name: calculator
description: 一个简单的计算器
version: "0.1.0"
author: EdgeCrab Team
compatibility:
  hermes: ">=0.1.0"
  edgecrab: ">=0.1.0"
requires_env: []
```

**`schemas.py`**:

```python
from pydantic import BaseModel, Field

class AddArgs(BaseModel):
    a: float = Field(description="第一个数字")
    b: float = Field(description="第二个数字")

class MultiplyArgs(BaseModel):
    a: float = Field(description="第一个数字")
    b: float = Field(description="第二个数字")
```

**`tools.py`**:

```python
from .schemas import AddArgs, MultiplyArgs
from hermes.constants import HERMES_TOOL_ERROR

def add(args):
    args = AddArgs(**args)
    return {"result": args.a + args.b}

def multiply(args):
    args = MultiplyArgs(**args)
    return {"result": args.a * args.b}
```

**`__init__.py`**:

```python
from .tools import add, multiply
from hermes.constants import HERMES_TOOL_ERROR

def register(agent):
    agent.tools.register_tool(
        name="add",
        description="将两个数字相加",
        arguments=AddArgs.model_json_schema(),
        tool_error=HERMES_TOOL_ERROR,
        handler=add
    )
    
    agent.tools.register_tool(
        name="multiply",
        description="将两个数字相乘",
        arguments=MultiplyArgs.model_json_schema(),
        tool_error=HERMES_TOOL_ERROR,
        handler=multiply
    )
```

**`SKILL.md`**:

```markdown
# 计算器技能

使用计算器工具进行数学运算。

## 可用工具

- `add`: 将两个数字相加
- `multiply`: 将两个数字相乘

## 示例

```
/add {"a": 5, "b": 3}
/multiply {"a": 10, "b": 2}
```
```

## 验证

```bash
edgecrab plugins install ./calculator
edgecrab plugins info calculator
edgecrab tools call add '{"a": 5, "b": 3}'
```

## 提示

1. 保持工具描述清晰简洁。
2. 使用 Pydantic 模式进行类型检查。
3. 始终返回 JSON 可序列化的结果。
4. 在 `requires_env` 中声明必需的环境变量。
5. 使用 `SKILL.md` 提供使用指南。
6. 测试你的插件与真实的 LLM 交互。