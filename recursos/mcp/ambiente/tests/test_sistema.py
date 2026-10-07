"""Leituras do /proc e do statvfs (na máquina de teste de verdade)."""

import os
import socket
import subprocess
import sys
import time

import sistema


def test_ocultar_segredos():
    casos = {
        "mysql -u root --password=hunter2 db": "mysql -u root --password=*** db",
        "app --token abc123 --verbose": "app --token *** --verbose",
        "app --api-key=XYZ": "app --api-key=***",
        "curl https://user:senha@exemplo.com/x": "curl https://***@exemplo.com/x",
        "env API_KEY=xyz OPENAI_API_KEY=abc python": "env API_KEY=*** OPENAI_API_KEY=*** python",
        "postgres: password=s3gredo user=x": "postgres: password=*** user=x",
        "ls -la /home": "ls -la /home",
    }
    for entrada, saida in casos.items():
        assert sistema.ocultar_segredos(entrada) == saida, entrada


def test_enderecos_do_proc_net():
    assert sistema._endereco("0100007F") == "127.0.0.1"
    assert sistema._endereco("00000000") == "0.0.0.0"
    assert sistema._endereco("0101A8C0") == "192.168.1.1"
    assert sistema._endereco("00000000000000000000000001000000") == "::1"
    assert sistema._endereco("0000000000000000FFFF00000100007F") == "::ffff:127.0.0.1"


def test_portas_acha_o_socket_deste_processo():
    tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    tcp.bind(("127.0.0.1", 0))
    tcp.listen()
    udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    udp.bind(("0.0.0.0", 0))
    try:
        portas = sistema.portas()
        meu_tcp = next(p for p in portas if p["protocolo"] == "tcp" and p["porta"] == tcp.getsockname()[1])
        assert meu_tcp["endereco"] == "127.0.0.1" and meu_tcp["alcance"] == "só local"
        assert meu_tcp["pid"] == os.getpid()
        meu_udp = next(p for p in portas if p["protocolo"] == "udp" and p["porta"] == udp.getsockname()[1])
        assert meu_udp["alcance"] == "todas as interfaces"
    finally:
        tcp.close()
        udp.close()


def test_portas_de_um_proc_net_falso(tmp_path):
    cabecalho = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n"
    (tmp_path / "tcp").write_text(
        cabecalho
        + "   0: 0100007F:1FF5 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 999991 1\n"
        + "   1: 0100007F:9C40 0100007F:1FF5 01 00000000:00000000 00:00000000 00000000  1000        0 999992 1\n"
    )
    (tmp_path / "udp6").write_text(
        cabecalho + "  10: 00000000000000000000000000000000:0035 00000000000000000000000000000000:0000 07 "
        "00000000:00000000 00:00000000 00000000     0        0 999993 2\n"
    )
    portas = sistema.portas(tmp_path)
    assert [(p["protocolo"], p["endereco"], p["porta"], p["alcance"]) for p in portas] == [
        ("tcp", "127.0.0.1", 8181, "só local"),
        ("udp", "::", 53, "todas as interfaces"),
    ], "a conexão estabelecida (01) não conta"


def test_processos_com_segredo_oculto():
    filho = subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(30)", "--token=segredo123", "senha=abc"]
    )
    try:
        time.sleep(0.2)
        todos = sistema.processos(maximo=100000)
        meu = next(p for p in todos if p.pid == filho.pid)
        assert "--token=***" in meu.comando and "senha=***" in meu.comando
        assert "segredo123" not in meu.comando and "abc" not in meu.comando.split("senha=")[1]
        assert meu.rss > 0 and meu.usuario
        assert todos == sorted(todos, key=lambda p: p.rss, reverse=True)
    finally:
        filho.kill()


def test_disco_memoria_e_carga(tmp_path):
    discos = sistema.discos(tmp_path)
    assert discos, "pelo menos um sistema de arquivos de verdade"
    for d in discos:
        assert d["total"] > 0 and 0 <= d["uso_pct"] <= 100
    assert sum(d["projeto"] for d in discos) == 1, "o disco do projeto é marcado"
    m = sistema.memoria()
    assert 0 < m["disponivel"] <= m["total"]
    c = sistema.carga()
    assert c["cpus"] >= 1 and c["ligado_segundos"] > 0
