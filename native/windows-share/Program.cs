using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using Microsoft.Windows.AppLifecycle;
using Windows.ApplicationModel.Activation;
using Windows.ApplicationModel.DataTransfer;
using Windows.ApplicationModel.DataTransfer.ShareTarget;
using Windows.Storage;
using Windows.Storage.Streams;

namespace ArcRelay.ShareTarget;

internal static class Program
{
    private const int MaximumFileCount = 256;
    private const ulong MaximumTotalBytes = 16UL * 1024 * 1024 * 1024 * 1024;

    [STAThread]
    private static void Main()
    {
        ApplicationConfiguration.Initialize();
        try
        {
            var activation = AppInstance.GetCurrent().GetActivatedEventArgs();
            if (activation?.Kind != ExtendedActivationKind.ShareTarget || activation.Data is not ShareTargetActivatedEventArgs shareArgs)
                return;
            HandleShareAsync(shareArgs).GetAwaiter().GetResult();
        }
        catch (Exception error)
        {
            MessageBox.Show(error.Message, "ArcRelay", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }

    private static async Task HandleShareAsync(ShareTargetActivatedEventArgs args)
    {
        var operation = args.ShareOperation;
        string? requestDirectory = null;
        var handedOff = false;
        try
        {
            if (!operation.Data.Contains(StandardDataFormats.StorageItems))
                throw new InvalidDataException(Text("分享内容不包含文件。", "The shared content does not contain files."));

            var peerCache = LoadPeerCache();
            var targetPeer = ResolveQuickLink(operation.QuickLinkId, peerCache)
                ?? DevicePicker.Choose(peerCache);
            operation.ReportStarted();
            var storageItems = await operation.Data.GetStorageItemsAsync();
            if (storageItems.Count is 0 or > MaximumFileCount)
                throw new InvalidDataException(Text("一次最多分享 256 个文件。", "Share between 1 and 256 files at a time."));
            if (storageItems.Any(item => item is not StorageFile))
                throw new InvalidDataException(Text("当前仅支持分享普通文件。", "ArcRelay currently accepts regular files only."));

            var files = storageItems.Cast<StorageFile>().ToArray();
            ulong totalBytes = 0;
            foreach (var file in files)
            {
                var properties = await file.GetBasicPropertiesAsync();
                checked { totalBytes += properties.Size; }
            }
            if (totalBytes > MaximumTotalBytes)
                throw new InvalidDataException(Text("所选文件超过传输大小限制。", "The selected files exceed the transfer size limit."));
            var requestId = Guid.NewGuid().ToString("D");
            requestDirectory = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                "ArcRelay", "system-share", "external", requestId);
            var filesDirectory = Path.Combine(requestDirectory, "files");
            Directory.CreateDirectory(filesDirectory);
            var staged = new List<NativeFile>(files.Length);
            foreach (var file in files)
            {
                var destination = UniqueDestination(filesDirectory, file.Name);
                await using var source = await file.OpenStreamForReadAsync();
                await using var target = new FileStream(destination, FileMode.CreateNew, FileAccess.Write, FileShare.None, 1024 * 1024, true);
                await source.CopyToAsync(target);
                await target.FlushAsync();
                target.Flush(flushToDisk: true);
                staged.Add(new NativeFile(destination));
            }
            operation.ReportDataRetrieved();

            var request = new NativeRequest(
                1,
                requestId,
                "windowsShareTarget",
                DateTimeOffset.UtcNow.ToUnixTimeMilliseconds(),
                targetPeer?.Id,
                staged);
            var requestPath = Path.Combine(requestDirectory, "request.json");
            var temporaryPath = requestPath + ".tmp";
            var requestBytes = JsonSerializer.SerializeToUtf8Bytes(request, JsonOptions);
            await using (var requestFile = new FileStream(temporaryPath, FileMode.CreateNew, FileAccess.Write, FileShare.None, 64 * 1024, FileOptions.Asynchronous | FileOptions.WriteThrough))
            {
                await requestFile.WriteAsync(requestBytes);
                await requestFile.FlushAsync();
                requestFile.Flush(flushToDisk: true);
            }
            File.Move(temporaryPath, requestPath);
            handedOff = true;
            LaunchArcRelay(requestPath);

            if (targetPeer is not null)
                await ReportWithQuickLink(operation, targetPeer);
            else
                operation.ReportCompleted();
        }
        catch (Exception error)
        {
            if (!handedOff && requestDirectory is not null)
            {
                try { Directory.Delete(requestDirectory, recursive: true); } catch { }
            }
            operation.ReportError(error.Message);
        }
    }

    private static IReadOnlyList<SharePeer> LoadPeerCache()
    {
        try
        {
            var path = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                "ArcRelay", "system-share", "peers.json");
            var cache = JsonSerializer.Deserialize<PeerCache>(File.ReadAllText(path), JsonOptions);
            return cache?.Peers ?? [];
        }
        catch
        {
            return [];
        }
    }

    private static SharePeer? ResolveQuickLink(string? quickLinkId, IReadOnlyList<SharePeer> peers)
    {
        const string prefix = "peer:";
        if (string.IsNullOrWhiteSpace(quickLinkId) || !quickLinkId.StartsWith(prefix, StringComparison.Ordinal))
            return null;
        var id = quickLinkId[prefix.Length..];
        return peers.FirstOrDefault(peer => string.Equals(peer.Id, id, StringComparison.Ordinal));
    }

    private static async Task ReportWithQuickLink(ShareOperation operation, SharePeer peer)
    {
        var quickLink = new QuickLink
        {
            Id = "peer:" + peer.Id,
            Title = peer.Name,
        };
        quickLink.SupportedFileTypes.Add("*");
        quickLink.SupportedDataFormats.Add(StandardDataFormats.StorageItems);
        var iconPath = Path.Combine(AppContext.BaseDirectory, "ArcRelay.png");
        if (File.Exists(iconPath))
        {
            var icon = await StorageFile.GetFileFromPathAsync(iconPath);
            quickLink.Thumbnail = RandomAccessStreamReference.CreateFromFile(icon);
        }
        operation.ReportCompleted(quickLink);
    }

    private static void LaunchArcRelay(string requestPath)
    {
        var installRoot = Directory.GetParent(AppContext.BaseDirectory.TrimEnd(Path.DirectorySeparatorChar))?.Parent?.FullName;
        if (installRoot is null)
            return;
        var executable = new[] { "arcrelay-desktop.exe", "ArcRelay.exe" }
            .Select(name => Path.Combine(installRoot, name))
            .FirstOrDefault(File.Exists);
        if (executable is null)
            return;
        Process.Start(new ProcessStartInfo
        {
            FileName = executable,
            UseShellExecute = false,
            Arguments = $"--arcrelay-share-request {QuoteArgument(requestPath)}",
        });
    }

    private static string UniqueDestination(string directory, string name)
    {
        var clean = string.Concat((string.IsNullOrWhiteSpace(name) ? "Shared file" : name)
            .Select(character => Path.GetInvalidFileNameChars().Contains(character) ? '_' : character));
        var candidate = Path.Combine(directory, clean);
        if (!File.Exists(candidate)) return candidate;
        var stem = Path.GetFileNameWithoutExtension(clean);
        var suffix = Path.GetExtension(clean);
        for (var index = 1; index < 10_000; index++)
        {
            candidate = Path.Combine(directory, $"{stem} ({index}){suffix}");
            if (!File.Exists(candidate)) return candidate;
        }
        return Path.Combine(directory, Guid.NewGuid().ToString("N") + suffix);
    }

    private static string QuoteArgument(string value) => "\"" + value.Replace("\"", "\\\"") + "\"";
    private static bool Chinese => CultureInfo.CurrentUICulture.Name.StartsWith("zh", StringComparison.OrdinalIgnoreCase);
    internal static string Text(string chinese, string english) => Chinese ? chinese : english;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
    };

    private sealed record NativeFile(string Path);
    private sealed record NativeRequest(int Version, string Id, string Source, long CreatedAtMs, string? TargetPeerId, IReadOnlyList<NativeFile> Files);
    private sealed record PeerCache(int Version, long UpdatedAtMs, IReadOnlyList<SharePeer> Peers);
    internal sealed record SharePeer(string Id, string Name, string Platform, string Model, bool Paired, long LastSeenAtMs);
}

internal sealed class DevicePicker : Form
{
    private readonly ComboBox _devices = new() { DropDownStyle = ComboBoxStyle.DropDownList, Dock = DockStyle.Top };
    private Program.SharePeer? SelectedPeer => _devices.SelectedItem is PeerChoice choice ? choice.Peer : null;

    private DevicePicker(IReadOnlyList<Program.SharePeer> peers)
    {
        Text = "ArcRelay";
        Width = 440;
        Height = 200;
        StartPosition = FormStartPosition.CenterScreen;
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        MinimizeBox = false;
        Padding = new Padding(20);
        var label = new Label
        {
            Text = Program.Text("发送到附近设备", "Send to a nearby device"),
            Font = new Font(SystemFonts.MessageBoxFont.FontFamily, 13, FontStyle.Bold),
            Dock = DockStyle.Top,
            Height = 36,
        };
        _devices.Items.Add(new PeerChoice(null, Program.Text("在 ArcRelay 中选择设备", "Choose a device in ArcRelay")));
        foreach (var peer in peers)
        {
            var details = string.Join(" · ", new[] { peer.Platform, peer.Model }.Where(value => !string.IsNullOrWhiteSpace(value)));
            _devices.Items.Add(new PeerChoice(peer, string.IsNullOrEmpty(details) ? peer.Name : $"{peer.Name} — {details}"));
        }
        _devices.SelectedIndex = peers.Count == 1 ? 1 : 0;
        var send = new Button { Text = Program.Text("继续", "Continue"), DialogResult = DialogResult.OK, Width = 96 };
        var cancel = new Button { Text = Program.Text("取消", "Cancel"), DialogResult = DialogResult.Cancel, Width = 96 };
        var buttons = new FlowLayoutPanel { Dock = DockStyle.Bottom, FlowDirection = FlowDirection.RightToLeft, Height = 42 };
        buttons.Controls.Add(send);
        buttons.Controls.Add(cancel);
        Controls.Add(buttons);
        Controls.Add(_devices);
        Controls.Add(label);
        AcceptButton = send;
        CancelButton = cancel;
    }

    internal static Program.SharePeer? Choose(IReadOnlyList<Program.SharePeer> peers)
    {
        if (peers.Count == 0) return null;
        using var picker = new DevicePicker(peers);
        if (picker.ShowDialog() != DialogResult.OK)
            throw new OperationCanceledException(Program.Text("分享已取消。", "Sharing was cancelled."));
        return picker.SelectedPeer;
    }

    private sealed record PeerChoice(Program.SharePeer? Peer, string Label)
    {
        public override string ToString() => Label;
    }
}
