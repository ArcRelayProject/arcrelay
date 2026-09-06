# ArcRelay 动作文本格式

动作文本使用 JSON，可从桌面端的“导入动作”窗口直接粘贴。支持单个动作、动作数组，以及标准动作包。

最简动作只需要名称和动作类型；`id`、图标、颜色、分组和排序均可省略，导入时会自动补全并生成新的 UUID。

```json
{
  "name": "打开下载目录",
  "action_type": {
    "type": "OpenPath",
    "path": "~/Downloads"
  }
}
```

标准动作包适合保存为 `*.arcrelay-actions.json` 并发布到 GitHub：

```json
{
  "format": "arcrelay-actions",
  "version": 1,
  "actions": [
    {
      "name": "锁定电脑",
      "group": "系统",
      "icon": "🔒",
      "color": "#2563EB",
      "action_type": {
        "type": "System",
        "operation": "lock_screen"
      }
    },
    {
      "name": "打开项目主页",
      "group": "常用",
      "action_type": {
        "type": "OpenUrl",
        "url": "https://github.com/"
      }
    }
  ]
}
```

导入限制为 2 MiB 和 512 个原子动作。Shell、AppleScript 与自定义程序路径可能执行本机代码，只应导入可信来源的内容。多步骤组合请使用“自动化工作流”。

## 本机全局快捷键

在动作详情中点击“添加全局快捷键”，或打开“编辑动作”，点击快捷键输入框后直接按下组合键并保存，无需手动填写文本。组合键必须包含 Ctrl、Alt 或 Command/Super；按 Backspace/Delete 或点击“清除”并保存可解除绑定，Tab 可以切换焦点。

录入后会立即检查其他动作、内置快捷键、已启用的剪贴板/截图功能以及其他已注册的 ArcRelay 快捷键（包括工作流），冲突时显示占用原因并禁止保存。保存时再次校验并尝试系统注册；若系统或其他应用导致注册失败，会保留原来的绑定并在编辑窗口显示原因。

`global_shortcut` 是可选的本机触发字段，与 `action_type: Hotkey` 中发送给系统的按键独立。ArcRelay 在后台运行时由 Rust 直接执行动作，关闭主窗口后仍可使用；退出应用后不再监听。需确认的动作会显示原生确认窗口。

绑定随动作持久化，启动时自动恢复。冲突会阻止保存并保留旧绑定；启动时被占用的绑定会在动作详情中显示错误，可修改或再次保存以重试。删除动作会注销绑定。导出的 JSON 保留该字段供参考，但导入时会清空，避免复制动作占用本机已有的快捷键。
