import { $, el, formField, invoke, state } from './core.js';
import { onLanguageChanged } from './lang.js';

const copy = {
  title: ['Git 仓库', 'Git repository'],
  provider: ['托管平台', 'Hosting provider'],
  host: ['主机名', 'Hostname'],
  login: ['验证 CLI 账号', 'Verify CLI account'],
  pending: ['正在处理…', 'Working…'],
  hint: ['本地密码库可以独立使用。连接仓库时使用此主机的 gh 或 glab 会话。', 'The local vault works independently. Connect a repository using the gh or glab session for this host.'],
  checkout: ['当前密码库尚未连接仓库。', 'This vault has no repository connection.'],
  name: ['新仓库名称', 'New repository name'],
  owner: ['仓库所有者', 'Repository owner'],
  consent: ['确认在此账号下创建私有仓库，初始文件为空的 secrets.json。', 'Create a private repository under this account with an empty secrets.json.'],
  create: ['创建私有仓库', 'Create private repository'],
  created: ['私有仓库已创建并绑定。点击同步后才会上传本地密文。', 'Private repository created and bound. Local ciphertext uploads when you choose Sync.'],
  createdUnbound: ['远端仓库已创建，请检查远端状态和本地绑定。', 'Repository created. Check the remote and local binding before retrying.'],
  createdPartial: ['远端仓库已创建，初始化或本地绑定尚未完成。请先检查该仓库。', 'Repository created. Initialization or local binding is incomplete. Check the repository before retrying.'],
  createFailed: ['创建或绑定未完成。请先检查平台上的仓库状态，再决定是否重试。', 'Creation or binding is incomplete. Check the repository on the host before retrying.'],
  verified: ['本次验证的账号', 'Account verified this session'],
  last: ['上次绑定的账号', 'Last bound account'],
  target: ['仓库和分支', 'Repository and branch'],
  bind: ['绑定此私有仓库', 'Bind this private repository'],
  unbind: ['解除 Ky 绑定', 'Unbind from Ky'],
  unbound: ['已解除绑定，CLI 仍保持登录。', 'Repository unbound. Your CLI session is still signed in.'],
  saved: ['已保存的绑定', 'Saved binding'],
  passwords: ['打开本地密码库', 'Open local vault'],
  sync: ['同步 secrets.json', 'Sync secrets.json'],
  scope: ['每次点击仅同步当前分支的 secrets.json。主密钥、别名、元数据和 Git 历史留在本机。同步时需要可用的本地主密钥。', 'Each sync updates only secrets.json on this branch. Master keys, aliases, metadata and Git history stay local. Sync requires a working local master key.'],
  same: ['本地与远端密文一致。', 'Local and remote ciphertext match.'],
  uploaded: ['密文已上传并回读确认。', 'Encrypted records uploaded and verified.'],
  downloaded: ['已更新本地密文。', 'Local encrypted records updated.'],
  bound: ['已绑定私有仓库。', 'Private repository bound.'],
  invalid: ['请输入有效主机名。', 'Enter a valid hostname.'],
  auth: ['无法验证 CLI 会话。请在终端登录后重新验证。', 'Could not verify the CLI session. Sign in from a terminal and verify again.'],
  private: ['项目未通过私有仓库验证。', 'The project could not be verified as private.'],
  binding: ['绑定已失效，请重新验证账号和仓库。', 'Verify the account and bind the repository again.'],
  unlock: ['请先解锁此库的本地主密钥。', 'Unlock the local master key for this vault first.'],
  cipher: ['文件含明文、未知格式或无法用本机主密钥验证的密文。', 'The file contains plaintext, an unknown format, or ciphertext that the local key cannot verify.'],
  conflict: ['本地与远端都已修改，已保留两边的内容。请先处理冲突。', 'Both copies changed. Resolve the conflict before syncing. Both versions are preserved.'],
  metadata: ['远端密文与本地完整性记录不一致，请核对后重试。', 'Remote ciphertext conflicts with local integrity records. Reconcile them before syncing.'],
  upload: ['远端未确认更新，请检查分支权限和最新版本。本地改动已保留。', 'Update unconfirmed. Check branch permissions and the latest revision. Local edits are preserved.'],
  verify: ['上传后的远端版本未能确认，请重新同步。', 'Could not verify the uploaded revision. Sync again to recheck.'],
  storage: ['本地文件读写失败。', 'Could not read or write local files.'],
  locked: ['请先返回已解锁的工作台。', 'Unlock the workspace first.'],
  gitlab: ['请求失败，请检查 CLI 会话、网络、仓库权限和分支上的 secrets.json。', 'Request failed. Check your CLI session, network, access and secrets.json on this branch.'],
};

let page;
const label = key => copy[key][state.lang === 'en' ? 1 : 0];

function paint(p) {
  const root = $('#account');
  if (!root || page !== p) return;
  const button = (key, run, disabled = false) => {
    const b = el('button', '', label(key));
    b.disabled = Boolean(p.busy || disabled);
    b.onclick = run;
    return b;
  };
  const note = key => el('p', 'account-note', label(key));
  const status = el('p', 'account-status', p.message ? label(p.message) : '');
  status.setAttribute('role', 'status');
  if (p.errorLocation) status.append(document.createTextNode(' · '+p.errorLocation));
  const focused = document.activeElement?.dataset?.accountField;
  const cursor = document.activeElement?.selectionStart;
  function field(tag, key, value) {
    const node = el(tag);
    node.dataset.accountField = key;
    node.value = value;
    node.disabled = Boolean(p.busy);
    return node;
  }
  const provider = field('select', 'provider', p.provider);
  for (const [value, name] of [['github','GitHub'], ['gitlab','GitLab']]) {
    const option = el('option','',name);
    option.value = value;
    provider.append(option);
  }
  provider.value = p.provider;
  provider.onchange = () => {
    p.provider = provider.value;
    p.host = t?.host && !['gitlab.com','github.com'].includes(t.host) ? t.host : p.provider + '.com';
    p.identity = null;
    p.consent = false;
    verify(p);
  };
  const host = field('input','host',p.host);
  host.autocapitalize = 'none';
  host.spellcheck = false;
  host.required = true;
  const body = el('div', 'account-body');
  body.append(el('h2','',label('title')), note('hint'), formField(label('provider'),provider),
    formField(label('host'),host), button('login', () => verify(p)));
  const passwords = button('passwords', () => window.dispatchEvent(new CustomEvent('ky-navigate',{detail:{view:'vault'}})));
  body.append(passwords);
  if (p.message === 'auth' && /^[a-z0-9.-]+$/i.test(p.host)) {
    body.append(el('code','account-project',(p.provider==='github'?'gh':'glab')+' auth login --hostname '+p.host+' --web'));
  }
  if (p.identity) body.append(el('p','',label('verified')+' · '+p.identity.username+' @ '+p.identity.host));
  const t=p.target;
  if (t?.path) {
    body.append(el('h3','',label('target')), el('p','account-project',t.host+'/'+t.path+' · '+t.branch));
    if (t.bound) body.append(el('p','',label('last')+' · '+t.username));
    const args={provider:t.provider,host:t.host,path:t.path,branch:t.branch};
    if (!t.bound) body.append(button('bind', () => request(p,'account_bind',
      {...args,provider:p.provider,username:p.identity.username}, result => {
        p.target={...result,saved:result}; p.message='bound';
      }), !p.identity || p.identity.host!==t.host));
    if (t.bound) body.append(button('sync', () => request(p,'account_sync',
      {...args,username:t.username}, result => {p.message=result;})));
  } else if (t && !t.saved) body.append(note('checkout'));
  if (t?.saved) {
    const {provider,host,path,branch,username}=t.saved;
    if (!t.bound) body.append(el('p','',label('saved')+' · '+username+' @ '+host+'/'+path+' · '+branch));
    body.append(button('unbind', () => request(p,'account_unbind',{provider,host,path,branch,username}, async () => {
      p.target=await invoke('account_status'); p.message='unbound';
    })));
  } else if (t && !t.path && p.identity) {
    const owner=field('input','owner',p.identity.username);
    owner.readOnly=true;
    const name=field('input','name',p.name);
    name.required=true;
    name.maxLength=100;
    name.spellcheck=false;
    const consent=el('input');
    consent.type='checkbox';
    consent.checked=p.consent;
    consent.disabled=Boolean(p.busy);
    const consentLabel=el('label','account-consent');
    consentLabel.append(consent,document.createTextNode(label('consent')));
    const create=button('create', () => request(p,'account_create',{
      provider:p.provider,host:p.identity.host,username:p.identity.username,name:p.name.trim()
    }, result => {
      p.target={...result,saved:result}; p.name=''; p.consent=false; p.message='created';
    }));
    const enable=()=>{create.disabled=Boolean(p.busy || !p.consent || !p.name.trim());};
    name.oninput=()=>{p.name=name.value;p.consent=false;consent.checked=false;enable();};
    consent.onchange=()=>{p.consent=consent.checked;enable();};
    enable();
    body.append(formField(label('owner'),owner),formField(label('name'),name),consentLabel,create);
  }
  host.oninput=()=>{
    p.host=host.value; p.consent=false;
    if (p.identity || p.message==='auth') {p.identity=null;p.message=null;paint(p);}
  };
  if (t) body.append(note('scope'));
  body.append(status);
  root.replaceChildren(body);
  if (focused) {
    const next=Array.from(body.querySelectorAll('[data-account-field]')).find(node=>node.dataset.accountField===focused);
    if (next && !next.disabled) {
      next.focus({preventScroll:true});
      if (cursor!=null && next.setSelectionRange) next.setSelectionRange(cursor,cursor);
    }
  }
}

async function request(p, command, args, accept) {
  if (p.busy) return;
  p.busy = true;
  p.message = 'pending';
  p.errorLocation=null;
  paint(p);
  try {
    const result = await invoke(command, args);
    if (p !== page) return;
    p.message = null;
    await accept(result);
  } catch (error) {
    if (p !== page) return;
    const code = String(error);
    p.message = code === 'host' ? 'invalid' : Object.hasOwn(copy, code) ? code : command==='account_create' ? 'createFailed' : 'gitlab';
    if (command==='account_create') {
      if (code.startsWith('已创建 ')) p.message='createdPartial';
      p.errorLocation=args.host+'/'+args.username+'/'+args.name;
    }
    if (command === 'account_login') p.identity = null;
  } finally {
    p.busy = false;
    paint(p);
  }
}

function verify(p) {
  p.identity=null;
  p.consent=false;
  p.host=p.host.trim().toLowerCase();
  return request(p,'account_login',{provider:p.provider,host:p.host}, result => {p.identity=result;});
}

export function renderAccount() {
  const root = $('#account');
  if (!root || state.locked) return;
  root.hidden = false;
  if (page) return paint(page);
  const p = {provider:'github',host:'github.com',name:'',consent:false,target:null,identity:null};
  page = p;
  return request(p, 'account_status', {}, result => {
    p.target = result;
    p.host=result.host || p.host;
    p.provider=result.provider || p.provider;
  }).then(() => {if (page===p && p.target) return verify(p);});
}

export function clearAccount() {
  page = null;
  const root = $('#account');
  if (root) { root.replaceChildren(); root.hidden = true; }
}

onLanguageChanged(() => { if (page) paint(page); });
