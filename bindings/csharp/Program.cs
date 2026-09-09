using System;
using System.Runtime.InteropServices;

internal static class Native
{
    private const string Library = "warpigs_core";

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    internal static extern uint wp_engine_abi_version();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    internal static extern uint wp_engine_configuration_count();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    internal static extern uint wp_engine_create(
        ulong populationSize,
        ulong maxPopulation,
        ulong seed,
        out IntPtr engine);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    internal static extern void wp_engine_destroy(IntPtr engine);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    internal static extern uint wp_engine_step(IntPtr engine, uint action);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    internal static extern ulong wp_engine_population(IntPtr engine);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    internal static extern ulong wp_engine_tick(IntPtr engine);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    internal static extern uint wp_engine_configuration_text(
        IntPtr engine,
        ulong index,
        byte[] output,
        nuint capacity);
}

internal sealed class WarPigsEngine : IDisposable
{
    private IntPtr _handle;

    public WarPigsEngine(ulong population, ulong maxPopulation, ulong seed)
    {
        var status = Native.wp_engine_create(population, maxPopulation, seed, out _handle);
        if (status != 0 || _handle == IntPtr.Zero)
            throw new InvalidOperationException($"native engine creation failed: {status}");
    }

    public ulong Population => _handle == IntPtr.Zero ? 0 : Native.wp_engine_population(_handle);
    public ulong Tick => _handle == IntPtr.Zero ? 0 : Native.wp_engine_tick(_handle);

    public string Configuration(ulong index)
    {
        EnsureOpen();
        var bytes = new byte[5];
        var status = Native.wp_engine_configuration_text(_handle, index, bytes, (nuint)bytes.Length);
        if (status != 0)
            throw new InvalidOperationException($"configuration lookup failed: {status}");
        var length = Array.IndexOf(bytes, (byte)0);
        if (length < 0) length = 4;
        return System.Text.Encoding.ASCII.GetString(bytes, 0, length);
    }

    public void Step(uint action)
    {
        EnsureOpen();
        var status = Native.wp_engine_step(_handle, action);
        if (status != 0)
            throw new InvalidOperationException($"step failed: {status}");
    }

    public void Dispose()
    {
        if (_handle == IntPtr.Zero) return;
        Native.wp_engine_destroy(_handle);
        _handle = IntPtr.Zero;
    }

    private void EnsureOpen()
    {
        if (_handle == IntPtr.Zero)
            throw new ObjectDisposedException(nameof(WarPigsEngine));
    }
}

internal static class Program
{
    private static void Main()
    {
        Console.WriteLine($"WarPigs ABI v{Native.wp_engine_abi_version()} / configs={Native.wp_engine_configuration_count()}");
        using var engine = new WarPigsEngine(16, 1000, 42);
        Console.WriteLine($"population={engine.Population}");
        engine.Step(0); // PREPARE
        engine.Step(1); // START
        Console.WriteLine($"tick={engine.Tick} first={engine.Configuration(0)}");
        engine.Step(2); // QUARANTINE
        engine.Step(3); // TERMINATE
    }
}
