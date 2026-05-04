# tidyfs

`tidyfs` 是一个用 Rust 写的小工具，用来清理空文件夹，以及拉平“只有最内层才有文件”的多层嵌套目录。

这个项目现在更偏向“直接可用”：

- 双击 `exe` 可以直接进入中文菜单
- 先扫描，再决定是否执行
- 扫描时显示单行动态进度
- 默认拉平规则是“按日期目录分组，保留日期目录内第一层，去掉更深的壳目录”

## 现在的行为

### 1. 清理空文件夹

会递归扫描你选中的根目录，从深到浅判断：

- 没有普通文件
- 没有非空子目录
- `desktop.ini`、`Thumbs.db`、`.DS_Store` 这类噪音文件默认不算有效内容

如果确认执行，就会删除这些空文件夹。

扫描空文件夹会使用多线程加速；执行删除时如果某个目录因为权限、占用或系统限制删不掉，工具会记录 `[FAILED]` 并继续处理后面的目录，不会因为一个失败项中断整批任务。Windows 下遇到只读属性时，会先尝试清掉只读属性再删除。

如果文件资源管理器正好停在某个已经被删除的空文件夹里，再访问它时 Windows 会提示路径不存在或位置不可用。这是正常现象，回到上层目录或按 `F5` 刷新即可。

### 2. 拉平目录

默认规则是：

- 你选择的目录下面，每个一级子目录都会作为一个整理单元，例如日期目录 `20250203`
- 在日期目录里面，保留第一层目录，例如 `project_alpha`
- 第一层目录下面所有只起传递作用的壳目录都会去掉
- 中间壳目录必须没有普通文件
- 移动成功后，才会尝试删除已经空掉的壳目录

例子：

```text
Downloads/
  20250203/
    cover.jpg
    project_alpha/
      archive_shell/
        video.mp4
```

会变成：

```text
Downloads/
  20250203/
    cover.jpg
    project_alpha/
      video.mp4
```

也就是 `20250203/project_alpha/archive_shell/video.mp4` 会变成 `20250203/project_alpha/video.mp4`。

普通多层目录也是同样规则：

```text
Downloads/
  20250101/
    a/
      b/
        c/
          file1.jpg
          file2.jpg
```

会变成：

```text
Downloads/
  20250101/
    a/
      file1.jpg
      file2.jpg
```

也就是 `20250101/a/b/c/file` 会变成 `20250101/a/file`。

如果中间有很多层，也会一次去掉：

```text
Downloads/
  20250101/
    a/
      b/
        x/
          y/
            c/
              photo.jpg
```

会变成：

```text
Downloads/
  20250101/
    a/
      photo.jpg
```

也就是 `20250101/a/b/x/y/c/photo.jpg` 会变成 `20250101/a/photo.jpg`。

如果同一个中间层下面有多个末端文件夹，也会一起处理：

```text
Downloads/
  20250101/
    a/
      b/
        c/
          c.txt
        d/
          d.txt
```

会变成：

```text
Downloads/
  20250101/
    a/
    c.txt
    d.txt
```

如果最内层里有很多文件，也会一起处理。  
如果目标位置有同名文件，会自动改名避免覆盖。  
如果结构更复杂，比如最内层下面还有复杂子目录，就不会贸然处理。

执行删除或拉平时，工具会在根目录下写入操作日志：

```text
.tidyfs-journals/
  tidyfs-时间戳.log
```

日志会记录已经完成的移动、删除文件、删除目录操作。它不是自动回滚功能，但可以用来核对实际发生了什么。

## 双击使用

直接双击 [target\release\tidyfs.exe](C:\Users\kekin\dev\文件夹清理\target\release\tidyfs.exe) 后，会看到中文菜单：

1. 扫描空文件夹，然后决定是否删除
2. 扫描可拉平目录，然后决定是否执行
3. 退出

交互流程是：

1. 选择功能
2. 弹出文件夹选择框
3. 扫描并显示进度
4. 预览结果
5. 询问是否执行
6. 执行完成后回到主菜单

不会做完一次就直接关闭窗口。

## 命令行使用

虽然双击已经能用，但命令行模式仍然保留。

### 扫描空文件夹

```powershell
tidyfs scan-empty "D:\Downloads"
```

也可以不传目录，程序会弹出目录选择框：

```powershell
tidyfs scan-empty
```

### 预览删除空文件夹

```powershell
tidyfs remove-empty "D:\Downloads" --dry-run
```

### 真正删除空文件夹

```powershell
tidyfs remove-empty "D:\Downloads" --apply
```

### 检测可拉平目录

```powershell
tidyfs detect-chains "D:\Photos"
```

### 默认拉平规则：保留首尾

```powershell
tidyfs flatten-single-chain "D:\Photos" --dry-run --mode keep-endpoints
```

### 其他模式

只提升一层：

```powershell
tidyfs flatten-single-chain "D:\Photos" --dry-run --mode one-level
```

一路压平到链起点：

```powershell
tidyfs flatten-single-chain "D:\Photos" --apply --mode collapse-chain
```

### 输出日志

```powershell
tidyfs detect-chains "D:\Archive" --log-file ".\tidyfs.log"
```

### 追加忽略文件名

```powershell
tidyfs remove-empty "D:\Archive" --dry-run --ignore-name ".nomedia" --ignore-name "ehthumbs.db"
```

## 配置文件

默认会尝试读取当前工作目录下的 [tidyfs.toml](C:\Users\kekin\dev\文件夹清理\tidyfs.toml)。

也可以显式指定：

```powershell
tidyfs --config ".\tidyfs.toml" flatten-single-chain "D:\Downloads" --dry-run
```

示例配置：

```toml
default_ignored = true
flatten_mode = "keep-endpoints"
ignore_names = ["desktop.ini", "Thumbs.db", ".nomedia"]
```

说明：

- `default_ignored = true` 会启用内置噪音文件名单
- `flatten_mode` 可选 `keep-endpoints`、`one-level`、`collapse-chain`
- `ignore_names` 会和命令行里的 `--ignore-name` 合并
- 命令行参数优先级高于配置文件

## 进度显示

为了避免“看起来像卡住”，现在会显示两类动态进度：

- 发现目录阶段
- 分析/执行阶段

扫描阶段只显示单行动态进度，不会持续刷很多日志。

默认会使用多线程加速：空文件夹扫描会并行检查目录内容；拉平扫描会并行分析目录；执行拉平时不同目标目录可以并行移动。同一个目标目录内部仍然串行处理，避免同名文件竞争导致覆盖或错名。

## 构建

开发调试：

```powershell
cargo run -- scan-empty
```

运行测试：

```powershell
cargo test
```

编译发布版：

```powershell
cargo build --release
```

生成的可执行文件在：

`target\release\tidyfs.exe`
