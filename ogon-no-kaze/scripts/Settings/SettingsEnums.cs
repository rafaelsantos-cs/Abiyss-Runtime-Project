namespace OgonNoKaze.Settings;

// Os valores numéricos destes enums vão para o settings.cfg e para os .tres:
// só acrescente itens no fim, nunca reordene.

public enum QualityPresetId { Low, Medium, High, Ultra, Custom }

public enum WindowModeOption { Windowed, Borderless, ExclusiveFullscreen }

public enum VSyncOption { Off, On, Adaptive }

public enum UpscalerOption { Bilinear, Fsr1, Fsr2 }

public enum AntiAliasingOption { Off, Fxaa, Smaa, Taa }

public enum ShadowQuality { Low, Medium, High, Ultra }

public enum FogQuality { Off, Low, Medium, High }

public enum SsaoQuality { Off, Low, Medium, High }

public enum GlobalIlluminationOption { Off, Ssil, Sdfgi }
