class ServerConfig {
  final String id;
  final String name;
  final String host;
  final String port;
  final String token;
  final bool useTLS;
  bool isOnline;
  DateTime? lastConnected;

  ServerConfig({
    required this.id,
    required this.name,
    required this.host,
    required this.port,
    required this.token,
    this.useTLS = false,
    this.isOnline = false,
    this.lastConnected,
  });

  String get baseUrl {
    final scheme = useTLS ? 'https' : 'http';
    return '$scheme://$host:$port';
  }

  String get wsUrl {
    final scheme = useTLS ? 'wss' : 'ws';
    return '$scheme://$host:$port';
  }

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'host': host,
        'port': port,
        'token': token,
        'useTLS': useTLS,
        'lastConnected': lastConnected?.toIso8601String(),
      };

  factory ServerConfig.fromJson(Map<String, dynamic> json) => ServerConfig(
        id: json['id'],
        name: json['name'],
        host: json['host'],
        port: json['port'],
        token: json['token'],
        useTLS: json['useTLS'] ?? false,
        lastConnected: json['lastConnected'] != null
            ? DateTime.parse(json['lastConnected'])
            : null,
      );

  ServerConfig copyWith({
    String? id,
    String? name,
    String? host,
    String? port,
    String? token,
    bool? useTLS,
    bool? isOnline,
    DateTime? lastConnected,
  }) {
    return ServerConfig(
      id: id ?? this.id,
      name: name ?? this.name,
      host: host ?? this.host,
      port: port ?? this.port,
      token: token ?? this.token,
      useTLS: useTLS ?? this.useTLS,
      isOnline: isOnline ?? this.isOnline,
      lastConnected: lastConnected ?? this.lastConnected,
    );
  }
}

class SystemInfo {
  final CpuInfo cpu;
  final MemoryInfo memory;
  final DiskInfo disk;
  final HostInfo host;
  final LoadInfo load;
  final NetworkInfo network;
  final String publicIp;
  final UfwInfo ufw;

  SystemInfo({
    required this.cpu,
    required this.memory,
    required this.disk,
    required this.host,
    required this.load,
    required this.network,
    required this.publicIp,
    required this.ufw,
  });

  factory SystemInfo.fromJson(Map<String, dynamic> json) => SystemInfo(
        cpu:      CpuInfo.fromJson(json['cpu']),
        memory:   MemoryInfo.fromJson(json['memory']),
        disk:     DiskInfo.fromJson(json['disk']),
        host:     HostInfo.fromJson(json['host']),
        load:     LoadInfo.fromJson(json['load']),
        network:  NetworkInfo.fromJson(json['network']),
        publicIp: json['publicIp'] ?? '',
        ufw:      UfwInfo.fromJson(json['ufw'] ?? {}),
      );
}

class UfwInfo {
  final bool installed;
  final bool enabled;
  final int ruleCount;

  UfwInfo({required this.installed, required this.enabled, required this.ruleCount});

  factory UfwInfo.fromJson(Map<String, dynamic> json) => UfwInfo(
        installed: json['installed'] ?? false,
        enabled:   json['enabled']   ?? false,
        ruleCount: json['ruleCount'] ?? 0,
      );
}

class CpuInfo {
  final List<double> percent;
  final String model;
  final int cores;

  CpuInfo({required this.percent, required this.model, required this.cores});

  factory CpuInfo.fromJson(Map<String, dynamic> json) {
    final pct = json['percent'];
    List<double> percentList = [];
    if (pct is List) {
      percentList = pct.map((e) => (e as num).toDouble()).toList();
    } else if (pct is num) {
      percentList = [pct.toDouble()];
    }
    return CpuInfo(
      percent: percentList,
      model: json['model'] ?? '',
      cores: json['cores'] ?? 0,
    );
  }

  double get avgPercent {
    if (percent.isEmpty) return 0;
    return percent.reduce((a, b) => a + b) / percent.length;
  }
}

class MemoryInfo {
  final int total;
  final int used;
  final int free;
  final double percent;

  MemoryInfo(
      {required this.total,
      required this.used,
      required this.free,
      required this.percent});

  factory MemoryInfo.fromJson(Map<String, dynamic> json) => MemoryInfo(
        total: json['total'] ?? 0,
        used: json['used'] ?? 0,
        free: json['free'] ?? 0,
        percent: (json['percent'] as num?)?.toDouble() ?? 0,
      );
}

class DiskInfo {
  final int total;
  final int used;
  final int free;
  final double percent;

  DiskInfo(
      {required this.total,
      required this.used,
      required this.free,
      required this.percent});

  factory DiskInfo.fromJson(Map<String, dynamic> json) => DiskInfo(
        total: json['total'] ?? 0,
        used: json['used'] ?? 0,
        free: json['free'] ?? 0,
        percent: (json['percent'] as num?)?.toDouble() ?? 0,
      );
}

class HostInfo {
  final String hostname;
  final String os;
  final String platform;
  final String platformVersion;
  final String kernelVersion;
  final int uptime;

  HostInfo({
    required this.hostname,
    required this.os,
    required this.platform,
    required this.platformVersion,
    required this.kernelVersion,
    required this.uptime,
  });

  factory HostInfo.fromJson(Map<String, dynamic> json) => HostInfo(
        hostname: json['hostname'] ?? '',
        os: json['os'] ?? '',
        platform: json['platform'] ?? '',
        platformVersion: json['platformVersion'] ?? '',
        kernelVersion: json['kernelVersion'] ?? '',
        uptime: json['uptime'] ?? 0,
      );
}

class LoadInfo {
  final double load1;
  final double load5;
  final double load15;

  LoadInfo(
      {required this.load1, required this.load5, required this.load15});

  factory LoadInfo.fromJson(Map<String, dynamic> json) => LoadInfo(
        load1: (json['load1'] as num?)?.toDouble() ?? 0,
        load5: (json['load5'] as num?)?.toDouble() ?? 0,
        load15: (json['load15'] as num?)?.toDouble() ?? 0,
      );
}

class NetworkInfo {
  final int bytesSent;
  final int bytesRecv;

  NetworkInfo({required this.bytesSent, required this.bytesRecv});

  factory NetworkInfo.fromJson(Map<String, dynamic> json) => NetworkInfo(
        bytesSent: json['bytesSent'] ?? 0,
        bytesRecv: json['bytesRecv'] ?? 0,
      );
}

class FileItem {
  final String name;
  final String path;
  final bool isDir;
  final int size;
  final String mode;
  final DateTime modTime;
  final bool isSymlink;
  final String? symlinkDest;

  FileItem({
    required this.name,
    required this.path,
    required this.isDir,
    required this.size,
    required this.mode,
    required this.modTime,
    this.isSymlink = false,
    this.symlinkDest,
  });

  factory FileItem.fromJson(Map<String, dynamic> json) => FileItem(
        name: json['name'] ?? '',
        path: json['path'] ?? '',
        isDir: json['isDir'] ?? false,
        size: json['size'] ?? 0,
        mode: json['mode'] ?? '',
        modTime: DateTime.tryParse(json['modTime'] ?? '') ?? DateTime.now(),
        isSymlink: json['isSymlink'] ?? false,
        symlinkDest: json['symlinkDest'],
      );

  String get formattedSize {
    if (isDir) return '--';
    if (size < 1024) return '${size}B';
    if (size < 1024 * 1024) return '${(size / 1024).toStringAsFixed(1)}KB';
    if (size < 1024 * 1024 * 1024) {
      return '${(size / (1024 * 1024)).toStringAsFixed(1)}MB';
    }
    return '${(size / (1024 * 1024 * 1024)).toStringAsFixed(1)}GB';
  }
}

class ProcessInfo {
  final int pid;
  final String name;
  final double cpu;
  final double memory;
  final String status;
  final String user;
  final String cmdline;

  ProcessInfo({
    required this.pid,
    required this.name,
    required this.cpu,
    required this.memory,
    required this.status,
    required this.user,
    required this.cmdline,
  });

  factory ProcessInfo.fromJson(Map<String, dynamic> json) => ProcessInfo(
        pid: json['pid'] ?? 0,
        name: json['name'] ?? '',
        cpu: (json['cpu'] as num?)?.toDouble() ?? 0,
        memory: (json['memory'] as num?)?.toDouble() ?? 0,
        status: json['status'] ?? '',
        user: json['user'] ?? '',
        cmdline: json['cmdline'] ?? '',
      );
}

// ── Systemd Service ───────────────────────────────────────────
class ServiceInfo {
  final String name;
  final String description;
  final String loadState;   // loaded / not-found
  final String activeState; // active / inactive / failed / activating
  final String subState;    // running / dead / exited / …
  final bool enabled;       // enabled / disabled
  final String runlevel;    // openrc: default / boot / sysinit; systemd: ''

  ServiceInfo({
    required this.name,
    required this.description,
    required this.loadState,
    required this.activeState,
    required this.subState,
    required this.enabled,
    this.runlevel = '',
  });

  bool get isRunning => activeState == 'active' && subState == 'running';
  bool get isFailed  => activeState == 'failed';

  factory ServiceInfo.fromJson(Map<String, dynamic> json) => ServiceInfo(
        name:        json['name']        ?? '',
        description: json['description'] ?? '',
        loadState:   json['loadState']   ?? '',
        activeState: json['activeState'] ?? '',
        subState:    json['subState']    ?? '',
        enabled:     json['enabled']     ?? false,
        runlevel:    json['runlevel']     ?? '',
      );

  Map<String, dynamic> toJson() => {
        'name':        name,
        'description': description,
        'loadState':   loadState,
        'activeState': activeState,
        'subState':    subState,
        'enabled':     enabled,
        'runlevel':    runlevel,
      };
}
