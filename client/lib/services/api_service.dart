import 'dart:io';
import 'package:dio/dio.dart';
import 'package:dio/io.dart';
import '../models/models.dart';

class ApiService {
  late Dio _dio;
  late ServerConfig _server;

  ApiService(ServerConfig server) {
    _server = server;
    _dio = Dio(BaseOptions(
      baseUrl: server.baseUrl,
      connectTimeout: const Duration(seconds: 10),
      receiveTimeout: const Duration(seconds: 30),
      headers: {
        'X-Token': server.token,
        'Content-Type': 'application/json',
      },
    ));

    // Allow self-signed certs for TLS
    (_dio.httpClientAdapter as IOHttpClientAdapter).createHttpClient = () {
      final client = HttpClient();
      client.badCertificateCallback = (cert, host, port) => true;
      return client;
    };
  }

  // ── Auth ──────────────────────────────────────────────
  Future<bool> verify() async {
    try {
      final res = await _dio.post('/api/auth/verify',
          data: {'token': _server.token});
      return res.statusCode == 200;
    } catch (_) {
      return false;
    }
  }

  // ── System ────────────────────────────────────────────
  Future<SystemInfo> getSystemInfo() async {
    final res = await _dio.get('/api/system/info');
    return SystemInfo.fromJson(res.data);
  }

  Future<List<ProcessInfo>> getProcesses() async {
    final res = await _dio.get('/api/system/processes');
    final list = res.data['processes'] as List;
    return list.map((e) => ProcessInfo.fromJson(e)).toList();
  }

  Future<void> killProcess(int pid) async {
    await _dio.delete('/api/system/processes/$pid');
  }

  // ── Files ─────────────────────────────────────────────
  Future<Map<String, dynamic>> listFiles(String path) async {
    final res = await _dio.get('/api/files', queryParameters: {'path': path});
    final items = (res.data['items'] as List)
        .map((e) => FileItem.fromJson(e))
        .toList();
    return {'path': res.data['path'], 'items': items};
  }

  Future<String> readFile(String path) async {
    final res =
        await _dio.get('/api/files/read', queryParameters: {'path': path});
    return res.data['content'] as String;
  }

  Future<void> writeFile(String path, String content) async {
    await _dio
        .post('/api/files/write', data: {'path': path, 'content': content});
  }

  Future<void> mkdir(String path) async {
    await _dio.post('/api/files/mkdir', data: {'path': path});
  }

  Future<void> rename(String oldPath, String newPath) async {
    await _dio.post('/api/files/rename',
        data: {'oldPath': oldPath, 'newPath': newPath});
  }

  Future<void> delete(String path) async {
    await _dio.delete('/api/files', data: {'path': path});
  }

  Future<void> touch(String path) async {
    await _dio.post('/api/files/touch', data: {'path': path});
  }

  Future<void> uploadFile(String destDir, String localPath,
      {Function(int, int)? onProgress}) async {
    final fileName = localPath.split('/').last;
    final formData = FormData.fromMap({
      'path': destDir,
      'file': await MultipartFile.fromFile(localPath, filename: fileName),
    });
    await _dio.post(
      '/api/files/upload',
      data: formData,
      onSendProgress:
          onProgress != null ? (sent, total) => onProgress(sent, total) : null,
      options: Options(
        headers: {'X-Token': _server.token},
        contentType: 'multipart/form-data',
      ),
    );
  }

  String downloadUrl(String path) {
    return '${_server.baseUrl}/api/files/download?path=${Uri.encodeComponent(path)}&token=${_server.token}';
  }

  // ── Services ──────────────────────────────────────────
  Future<Map<String, dynamic>> getServices() async {
    final res = await _dio.get('/api/services');
    final list = res.data['services'] as List;
    return {
      'initSystem': res.data['initSystem'] as String? ?? 'unknown',
      'services': list.map((e) => ServiceInfo.fromJson(e as Map<String, dynamic>)).toList(),
    };
  }

  Future<void> serviceAction(String name, String action) async {
    await _dio.post('/api/services/$name/action', data: {'action': action});
  }

  Future<Map<String, String>> getServiceUnit(String name) async {
    final res = await _dio.get('/api/services/$name/unit');
    return {
      'content': res.data['content'] as String? ?? '',
      'initSystem': res.data['initSystem'] as String? ?? 'unknown',
    };
  }

  Future<void> createService(Map<String, dynamic> data) async {
    await _dio.post('/api/services', data: data);
  }

  Future<void> updateService(String name, Map<String, dynamic> data) async {
    await _dio.put('/api/services/$name', data: data);
  }

  Future<void> deleteService(String name) async {
    await _dio.delete('/api/services/$name');
  }
}
